//! Read-only configuration export of a stopped source deployment.
//!
//! The command exists to carry the complete portable configuration, including
//! the personal MCP OAuth grants, off an old PostgreSQL deployment without
//! starting or upgrading it: it reads the source through a strictly read-only
//! pool, fences concurrent runtime sessions with the same advisory locks a
//! migration uses, refuses to run while any node is live, and encodes the
//! archive with the shipped passphrase container. Deployment-only state such as
//! the raw object-store configuration and the coordination rows never enters
//! the snapshot.
//!
//! Nothing here starts a node: there is no identity, listener, migration,
//! registry admission, audit write, Valkey access, or upstream request. The
//! database URL and the master secret come from the node configuration or the
//! environment, and the passphrase comes from a file or standard input; the URL
//! and every secret stay out of the report.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

use crate::cli::ExportConfigArgs;
use crate::db::config_repository::archive::{
    ARCHIVE_FORMAT_VERSION, MAX_PASSPHRASE_LEN, encode_archive, validate_passphrase,
};
use crate::db::config_repository::snapshot::legacy::LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY;
use crate::db::config_repository::{ConfigRepository, build_config_snapshot};
use crate::db::gate::ExportFence;
use crate::db::{SHARED_VALKEY_SETTINGS_KEY, UserStore};
use crate::relay_secrets::RelaySecretManager;

/// Bound on the wait for the export fences.
const FENCE_WAIT: Duration = Duration::from_secs(10);
/// The export needs one worker connection only; the helper caps the pool at two.
const READ_ONLY_POOL_CONNECTIONS: u32 = 2;
/// Upper bound on any passphrase input; the archive limit is far below it.
const PASSPHRASE_READ_LIMIT: u64 = (MAX_PASSPHRASE_LEN as u64) * 4;

/// Run `maintenance export-config`.
pub async fn run_export_config(args: ExportConfigArgs) -> Result<()> {
    let config =
        crate::config::read_node_config().map_err(|_| anyhow!("invalid node configuration"))?;
    let database_url = config.database_url.trim();
    if database_url.is_empty() {
        bail!("a source database URL is required for export-config");
    }
    let master = config
        .master_secret
        .as_ref()
        .ok_or_else(|| anyhow!("a master secret is required for export-config"))?;
    let manager = RelaySecretManager::from_key_bytes(*master.as_bytes());
    let passphrase = read_passphrase(&args)?;

    let pool = crate::db::connect_read_only(database_url, READ_ONLY_POOL_CONNECTIONS)
        .await
        .map_err(|error| anyhow!("failed to open the read-only source pool: {error}"))?;
    let outcome = export_under_fence(database_url, &pool, &manager, &passphrase, &args.out).await;
    pool.close().await;
    outcome
}

/// Take the fences, export, and always close the control session afterwards.
async fn export_under_fence(
    database_url: &str,
    pool: &sqlx::PgPool,
    manager: &RelaySecretManager,
    passphrase: &str,
    out: &Path,
) -> Result<()> {
    let fence = ExportFence::acquire(database_url, FENCE_WAIT).await?;
    let outcome = export_locked(pool, manager, passphrase, out).await;
    fence.release().await;
    outcome
}

async fn export_locked(
    pool: &sqlx::PgPool,
    manager: &RelaySecretManager,
    passphrase: &str,
    out: &Path,
) -> Result<()> {
    let live = crate::db::count_live_workers(pool, None)
        .await
        .map_err(|error| unsupported_source_schema("the worker registry", error))?;
    if live > 0 {
        bail!(
            "refusing to export: {live} live registered worker(s); stop every node of this \
             deployment first"
        );
    }

    let revision_before = crate::db::config_revision::read_revision(pool)
        .await
        .map_err(|error| unsupported_source_schema("the configuration revision", error))?;

    let repository = ConfigRepository::postgres(pool);
    let users = UserStore::postgres(pool);
    let snapshot = build_config_snapshot(&repository, &users, Some(manager))
        .await
        .map_err(|error| unsupported_source_schema("the configuration tables", error))?;
    if snapshot
        .domains
        .settings
        .iter()
        .any(|setting| setting.key == SHARED_VALKEY_SETTINGS_KEY)
    {
        // The coordination row belongs to the deployment, never to an archive.
        bail!("refusing to export: the snapshot carries the shared Valkey coordination row");
    }
    if snapshot
        .domains
        .settings
        .iter()
        .any(|setting| setting.key == LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY)
    {
        // The source reader excludes this deployment row; a snapshot that
        // still carries it must not become a portable archive.
        bail!("refusing to export: the snapshot carries the raw object store setting");
    }
    let backend = snapshot.manifest.backend_kind;
    let payload_format = snapshot.manifest.format_version;
    let domain_counts: Vec<(String, usize)> = snapshot
        .manifest
        .domains
        .iter()
        .map(|domain| (domain.name.clone(), domain.records))
        .collect();
    let grant_count = snapshot.domains.mcp_oauth.len();

    let archive = encode_archive(passphrase, backend, snapshot)
        .map_err(|_| anyhow!("failed to encode the configuration archive"))?;

    let revision_after = crate::db::config_revision::read_revision(pool)
        .await
        .map_err(|error| unsupported_source_schema("the configuration revision", error))?;
    if revision_after != revision_before {
        bail!(
            "the source configuration changed during the export (revision {revision_before} then \
             {revision_after}); stop every node and run the export again"
        );
    }

    write_archive(out, &archive.bytes)?;

    println!("Configuration export complete.");
    println!("backend: {}", backend.as_str());
    println!("container format: {ARCHIVE_FORMAT_VERSION}");
    println!("payload format: {payload_format}");
    println!("config revision: {revision_before}");
    println!("payload fingerprint: {}", archive.payload_fingerprint);
    println!("archive bytes: {}", archive.bytes.len());
    println!("personal MCP OAuth grants: {grant_count}");
    for (name, records) in domain_counts {
        println!("domain {name}: {records} record(s)");
    }
    println!("output: {out}", out = out.display());
    Ok(())
}

/// Map a source read failure to the supported-layout message: the export never
/// migrates, downgrades, or repairs the schema it reads.
fn unsupported_source_schema(part: &str, error: anyhow::Error) -> anyhow::Error {
    anyhow!(
        "the source database is not the supported v1.43 layout: {part} could not be read \
         ({error}); the export never migrates or repairs a source deployment"
    )
}

/// Read the passphrase from exactly one bounded source and validate it.
fn read_passphrase(args: &ExportConfigArgs) -> Result<String> {
    let raw = match (args.passphrase_file.as_deref(), args.passphrase_stdin) {
        (Some(path), false) => read_bounded_file(path)?,
        (None, true) => read_bounded_stdin()?,
        _ => bail!("exactly one of --passphrase-file or --passphrase-stdin is required"),
    };
    let passphrase = strip_one_line_ending(&raw);
    validate_passphrase(passphrase)
        .map_err(|_| anyhow!("the archive passphrase does not meet the required length bounds"))?;
    Ok(passphrase.to_string())
}

fn read_bounded_file(path: &Path) -> Result<String> {
    let file =
        std::fs::File::open(path).map_err(|_| anyhow!("failed to open the passphrase file"))?;
    decode_passphrase_input(file, "the passphrase file")
}

fn read_bounded_stdin() -> Result<String> {
    decode_passphrase_input(std::io::stdin().lock(), "the standard input")
}

fn decode_passphrase_input(reader: impl Read, source: &str) -> Result<String> {
    let mut buffer = Vec::new();
    reader
        .take(PASSPHRASE_READ_LIMIT + 1)
        .read_to_end(&mut buffer)
        .map_err(|_| anyhow!("failed to read {source}"))?;
    if buffer.len() as u64 > PASSPHRASE_READ_LIMIT {
        bail!("{source} is too long for an archive passphrase");
    }
    String::from_utf8(buffer).map_err(|_| anyhow!("{source} is not valid UTF-8"))
}

/// Strip exactly one trailing line ending. Nothing else is trimmed, so a
/// passphrase that deliberately ends in a space keeps it.
fn strip_one_line_ending(raw: &str) -> &str {
    let stripped = raw.strip_suffix("\r\n").or_else(|| raw.strip_suffix('\n'));
    stripped.unwrap_or(raw)
}

/// Create the archive without ever clobbering an existing file.
fn write_archive(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            anyhow!(
                "refusing to overwrite the existing output path {}",
                path.display()
            )
        } else {
            anyhow!(
                "failed to create the archive output file {}",
                path.display()
            )
        }
    })?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        // Only the file this command just created is removed.
        let _ = std::fs::remove_file(path);
        return Err(anyhow!(
            "failed to write the archive output file {}; the partial file was removed",
            path.display()
        ))
        .with_context(|| error.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decode_passphrase_input, strip_one_line_ending};

    #[test]
    fn exactly_one_line_ending_is_stripped() {
        assert_eq!(strip_one_line_ending("passphrase\n"), "passphrase");
        assert_eq!(strip_one_line_ending("passphrase\r\n"), "passphrase");
        assert_eq!(strip_one_line_ending("passphrase"), "passphrase");
        assert_eq!(strip_one_line_ending("passphrase\n\n"), "passphrase\n");
        assert_eq!(strip_one_line_ending("passphrase "), "passphrase ");
        assert_eq!(strip_one_line_ending(" passphrase"), " passphrase");
        assert_eq!(strip_one_line_ending(""), "");
    }

    #[test]
    fn oversized_and_non_utf8_input_is_rejected_without_echoing_it() {
        let long = vec![b'a'; super::PASSPHRASE_READ_LIMIT as usize + 1];
        let error = decode_passphrase_input(long.as_slice(), "the test input")
            .expect_err("an oversized passphrase must be rejected");
        assert!(error.to_string().contains("too long"), "{error}");
        let invalid = [0xff_u8, 0xfe];
        let error = decode_passphrase_input(invalid.as_slice(), "the test input")
            .expect_err("invalid UTF-8 must be rejected");
        assert!(error.to_string().contains("not valid UTF-8"), "{error}");
    }
}
