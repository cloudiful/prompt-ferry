//! Read-only schema and migration admission in front of every gate write.
//!
//! The gate must refuse an initialized legacy database before it claims a
//! draining generation, creates the gate table or runs migrations. This module
//! reads only catalog metadata on the already claimed control session and
//! decides from that evidence; it never creates an extension, a baseline or a
//! migration row.

use std::collections::BTreeSet;

use anyhow::{Result, anyhow, bail};
use sqlx::{PgConnection, Row};

use super::super::connection::node_migrator;

/// The eight request-family relations of the TimescaleDB layout.
pub const REQUEST_HYPERTABLES: [&str; 8] = [
    "request_records",
    "request_record_content",
    "request_record_raw_payloads",
    "request_record_block_refs",
    "usage_prompt_blocks",
    "request_record_assistant_artifacts",
    "request_record_tool_calls",
    "request_record_replay_snapshots",
];

/// The minimum TimescaleDB minor release that ships the native columnstore API.
const MIN_TIMESCALE_MAJOR: u64 = 2;
const MIN_TIMESCALE_MINOR: u64 = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuardMode {
    Join,
    Upgrade,
    Bootstrap,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedMigration {
    pub version: i64,
    pub checksum: Vec<u8>,
    pub success: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedMigration {
    pub version: i64,
    pub checksum: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hypertable {
    pub name: String,
    pub time_column: String,
    pub daily: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence {
    pub has_migrations_table: bool,
    pub has_gate_table: bool,
    pub request_relations: Vec<String>,
    pub applied: Vec<AppliedMigration>,
    pub hypertables: Vec<Hypertable>,
    pub timescale_version: Option<String>,
    pub timescale_license: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TimescaleSupport {
    Missing,
    Unlicensed(Option<String>),
    TooOld(String),
    Ready,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaAdmission {
    FreshBootstrap,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaRefusal {
    Timescale(TimescaleSupport),
    LegacyRequestSchema {
        present: Vec<String>,
    },
    PartialApplicationSchema {
        has_migrations_table: bool,
        has_gate_table: bool,
        present: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MigrationRefusal {
    UnknownApplied { version: i64 },
    MissingEmbedded { version: i64 },
    ChecksumMismatch { version: i64 },
    Dirty { version: i64 },
    PendingOnJoin { versions: Vec<i64> },
}

/// Inspect the claimed control session and refuse an unsupported schema before
/// any gate write.
pub async fn guard(
    connection: &mut PgConnection,
    mode: GuardMode,
    gate_row_present: bool,
) -> Result<()> {
    let evidence = inspect(connection).await?;
    let admission = decide_schema(&evidence, gate_row_present).map_err(schema_error)?;
    match (mode, admission) {
        (GuardMode::Bootstrap, SchemaAdmission::FreshBootstrap) => return Ok(()),
        (GuardMode::Bootstrap, SchemaAdmission::Existing) => {
            bail!("the node gate row is missing from a completed schema")
        }
        (_, SchemaAdmission::FreshBootstrap) => {
            bail!("the node gate row exists without an initialized application schema")
        }
        (_, SchemaAdmission::Existing) => {}
    }
    let embedded = embedded_migrations();
    decide_migrations(mode, &evidence.applied, &embedded).map_err(migration_error)
}

async fn inspect(connection: &mut PgConnection) -> Result<Evidence> {
    let inventory = sqlx::query(include_str!("../../sql/node_gate/schema_inventory.sql"))
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| read_failure())?;
    let has_migrations_table: bool = inventory
        .try_get("has_migrations_table")
        .map_err(|_| read_failure())?;
    let has_gate_table: bool = inventory
        .try_get("has_gate_table")
        .map_err(|_| read_failure())?;
    let request_relations: Vec<String> = inventory
        .try_get("request_relations")
        .map_err(|_| read_failure())?;

    let timescale = sqlx::query(include_str!("../../sql/node_gate/read_timescale.sql"))
        .fetch_one(&mut *connection)
        .await
        .map_err(|_| read_failure())?;
    let timescale_version: Option<String> = timescale
        .try_get("installed_version")
        .map_err(|_| read_failure())?;
    let timescale_license: Option<String> =
        timescale.try_get("license").map_err(|_| read_failure())?;

    let hypertables = if timescale_version.is_some() {
        let rows = sqlx::query(include_str!("../../sql/node_gate/read_hypertables.sql"))
            .fetch_all(&mut *connection)
            .await
            .map_err(|_| read_failure())?;
        rows.into_iter()
            .map(|row| {
                Ok(Hypertable {
                    name: row.try_get("hypertable_name").map_err(|_| read_failure())?,
                    time_column: row.try_get("column_name").map_err(|_| read_failure())?,
                    daily: row.try_get("is_daily").map_err(|_| read_failure())?,
                })
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    let applied = if has_migrations_table {
        let rows = sqlx::query(include_str!("../../sql/node_gate/read_migrations.sql"))
            .fetch_all(&mut *connection)
            .await
            .map_err(|_| read_failure())?;
        rows.into_iter()
            .map(|row| {
                Ok(AppliedMigration {
                    version: row.try_get("version").map_err(|_| read_failure())?,
                    checksum: row.try_get("checksum").map_err(|_| read_failure())?,
                    success: row.try_get("success").map_err(|_| read_failure())?,
                })
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    Ok(Evidence {
        has_migrations_table,
        has_gate_table,
        request_relations,
        applied,
        hypertables,
        timescale_version,
        timescale_license,
    })
}

fn read_failure() -> anyhow::Error {
    anyhow!("node schema admission read failed")
}

/// Parse an installed TimescaleDB version such as `2.32.0`.
pub fn parse_timescale_version(raw: &str) -> Option<(u64, u64, u64)> {
    let mut parts = raw.trim().split('.');
    let major = numeric_prefix(parts.next()?)?;
    let minor = numeric_prefix(parts.next()?)?;
    let patch = parts.next().and_then(numeric_prefix).unwrap_or(0);
    Some((major, minor, patch))
}

fn numeric_prefix(raw: &str) -> Option<u64> {
    let digits = raw.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    raw[..digits].parse().ok()
}

/// Classify the installed extension, TSL license and version.
pub fn classify_timescale(
    installed_version: Option<&str>,
    license: Option<&str>,
) -> TimescaleSupport {
    let Some(installed_version) = installed_version else {
        return TimescaleSupport::Missing;
    };
    if !license.is_some_and(|license| license.trim().eq_ignore_ascii_case("timescale")) {
        return TimescaleSupport::Unlicensed(license.map(str::to_owned));
    }
    let Some((major, minor, _)) = parse_timescale_version(installed_version) else {
        return TimescaleSupport::TooOld(installed_version.to_owned());
    };
    if major < MIN_TIMESCALE_MAJOR || (major == MIN_TIMESCALE_MAJOR && minor < MIN_TIMESCALE_MINOR)
    {
        return TimescaleSupport::TooOld(installed_version.to_owned());
    }
    TimescaleSupport::Ready
}

/// Decide whether the observed application schema may enter a gate write.
pub fn decide_schema(
    evidence: &Evidence,
    gate_row_present: bool,
) -> Result<SchemaAdmission, SchemaRefusal> {
    let support = classify_timescale(
        evidence.timescale_version.as_deref(),
        evidence.timescale_license.as_deref(),
    );
    if support != TimescaleSupport::Ready {
        return Err(SchemaRefusal::Timescale(support));
    }

    let expected: BTreeSet<&str> = REQUEST_HYPERTABLES.into_iter().collect();
    let present: BTreeSet<&str> = evidence
        .request_relations
        .iter()
        .map(String::as_str)
        .collect();
    let blank = !evidence.has_migrations_table && !evidence.has_gate_table && present.is_empty();
    if blank {
        return Ok(SchemaAdmission::FreshBootstrap);
    }

    let hypertables: BTreeSet<&str> = evidence
        .hypertables
        .iter()
        .filter(|table| table.time_column == "created_at" && table.daily)
        .map(|table| table.name.as_str())
        .collect();
    let complete = evidence.has_migrations_table
        && evidence.has_gate_table
        && gate_row_present
        && present == expected
        && hypertables == expected;
    if complete {
        return Ok(SchemaAdmission::Existing);
    }

    if present.iter().any(|relation| expected.contains(relation)) {
        return Err(SchemaRefusal::LegacyRequestSchema {
            present: present.into_iter().map(str::to_owned).collect(),
        });
    }
    Err(SchemaRefusal::PartialApplicationSchema {
        has_migrations_table: evidence.has_migrations_table,
        has_gate_table: evidence.has_gate_table,
        present: present.into_iter().map(str::to_owned).collect(),
    })
}

/// Compare applied migrations with the embedded UP migrations.
///
/// A strict prefix of the embedded migrations is the only pending state that is
/// allowed, and only for a normal upgrade or a fresh bootstrap. A join must find
/// the exact embedded set.
pub fn decide_migrations(
    mode: GuardMode,
    applied: &[AppliedMigration],
    embedded: &[EmbeddedMigration],
) -> Result<(), MigrationRefusal> {
    let mut embedded: Vec<&EmbeddedMigration> = embedded.iter().collect();
    embedded.sort_by_key(|migration| migration.version);
    let mut applied: Vec<&AppliedMigration> = applied.iter().collect();
    applied.sort_by_key(|migration| migration.version);

    for migration in &applied {
        let Some(shipped) = embedded
            .iter()
            .find(|shipped| shipped.version == migration.version)
        else {
            return Err(MigrationRefusal::UnknownApplied {
                version: migration.version,
            });
        };
        if !migration.success {
            return Err(MigrationRefusal::Dirty {
                version: migration.version,
            });
        }
        if migration.checksum != shipped.checksum {
            return Err(MigrationRefusal::ChecksumMismatch {
                version: migration.version,
            });
        }
    }

    for (index, shipped) in embedded.iter().enumerate() {
        match applied.get(index) {
            Some(recorded) if recorded.version == shipped.version => {}
            Some(recorded) if recorded.version > shipped.version => {
                return Err(MigrationRefusal::MissingEmbedded {
                    version: shipped.version,
                });
            }
            Some(recorded) => {
                return Err(MigrationRefusal::UnknownApplied {
                    version: recorded.version,
                });
            }
            None => {
                if mode != GuardMode::Join {
                    break;
                }
            }
        }
    }

    if applied.len() < embedded.len() && mode == GuardMode::Join {
        return Err(MigrationRefusal::PendingOnJoin {
            versions: embedded[applied.len()..]
                .iter()
                .map(|migration| migration.version)
                .collect(),
        });
    }
    if applied.len() > embedded.len()
        && let Some(extra) = applied.get(embedded.len())
    {
        return Err(MigrationRefusal::UnknownApplied {
            version: extra.version,
        });
    }
    Ok(())
}

fn embedded_migrations() -> Vec<EmbeddedMigration> {
    node_migrator()
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| EmbeddedMigration {
            version: migration.version,
            checksum: migration.checksum.to_vec(),
        })
        .collect()
}

fn schema_error(refusal: SchemaRefusal) -> anyhow::Error {
    match refusal {
        SchemaRefusal::Timescale(TimescaleSupport::Missing) => anyhow!(
            "TimescaleDB is not installed: install the timescale/timescaledb:2.32.0-pg18 \
             image (or CREATE EXTENSION timescaledb once as a superuser) before starting; \
             prompt-ferry never creates the extension automatically"
        ),
        SchemaRefusal::Timescale(TimescaleSupport::Unlicensed(license)) => anyhow!(
            "TimescaleDB license is {}: the columnstore and continuous aggregates require \
             the TimescaleDB TSL license; use the official non-OSS timescaledb image",
            license.as_deref().unwrap_or("not set")
        ),
        SchemaRefusal::Timescale(TimescaleSupport::TooOld(version)) => anyhow!(
            "TimescaleDB {version} is installed: prompt-ferry requires TimescaleDB >= 2.32.0 \
             for the native columnstore; use the timescale/timescaledb:2.32.0-pg18 image"
        ),
        SchemaRefusal::Timescale(TimescaleSupport::Ready) => unreachable!(),
        SchemaRefusal::LegacyRequestSchema { present } => anyhow!(
            "the request-family schema is not a TimescaleDB hypertable layout (present: {}). \
             This build requires a fresh database initialized as eight TimescaleDB hypertables \
             with one-day chunks. Export the configuration from the old database and restore it \
             into the fresh database; the old database is left unchanged",
            summarize(&present)
        ),
        SchemaRefusal::PartialApplicationSchema {
            has_migrations_table,
            has_gate_table,
            present,
        } => anyhow!(
            "the application schema is partial (migration history: {has_migrations_table}, \
             node gate: {has_gate_table}, request-family relations: {}); refusing to infer \
             ownership. Use a fresh database and restore the configuration from an export",
            summarize(&present)
        ),
    }
}

fn summarize(names: &[String]) -> String {
    const LIMIT: usize = 8;
    if names.len() <= LIMIT {
        return names.join(", ");
    }
    format!(
        "{} and {} more",
        names[..LIMIT].join(", "),
        names.len() - LIMIT
    )
}

fn migration_error(refusal: MigrationRefusal) -> anyhow::Error {
    match refusal {
        MigrationRefusal::UnknownApplied { version } => {
            anyhow!("the database has applied migration {version}, which this build does not ship")
        }
        MigrationRefusal::MissingEmbedded { version } => anyhow!(
            "the database is missing shipped migration {version} while later migrations are applied"
        ),
        MigrationRefusal::ChecksumMismatch { version } => anyhow!(
            "applied migration {version} does not match the shipped checksum; refusing to continue"
        ),
        MigrationRefusal::Dirty { version } => {
            anyhow!("migration {version} is recorded as failed; operator intervention is required")
        }
        MigrationRefusal::PendingOnJoin { versions } => anyhow!(
            "the node gate is ready for this version but the database has pending migrations \
             ({}); run a normal upgrade instead of a same-version join",
            versions
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded(version: i64, checksum: &[u8]) -> EmbeddedMigration {
        EmbeddedMigration {
            version,
            checksum: checksum.to_vec(),
        }
    }

    fn applied(version: i64, checksum: &[u8], success: bool) -> AppliedMigration {
        AppliedMigration {
            version,
            checksum: checksum.to_vec(),
            success,
        }
    }

    fn hypertables() -> Vec<Hypertable> {
        REQUEST_HYPERTABLES
            .into_iter()
            .map(|name| Hypertable {
                name: name.to_owned(),
                time_column: "created_at".to_owned(),
                daily: true,
            })
            .collect()
    }

    fn evidence() -> Evidence {
        Evidence {
            has_migrations_table: false,
            has_gate_table: false,
            request_relations: Vec::new(),
            applied: Vec::new(),
            hypertables: Vec::new(),
            timescale_version: Some("2.32.0".to_owned()),
            timescale_license: Some("timescale".to_owned()),
        }
    }

    #[test]
    fn timescale_versions_parse_and_require_the_232_floor() {
        assert_eq!(parse_timescale_version("2.32.0"), Some((2, 32, 0)));
        assert_eq!(parse_timescale_version(" 2.33.1 "), Some((2, 33, 1)));
        assert_eq!(parse_timescale_version("2.32.0-dev"), Some((2, 32, 0)));
        assert_eq!(parse_timescale_version("nope"), None);
        assert_eq!(
            classify_timescale(Some("2.32.0"), Some("timescale")),
            TimescaleSupport::Ready
        );
        assert_eq!(
            classify_timescale(Some("2.32.0"), Some(" Timescale ")),
            TimescaleSupport::Ready
        );
        assert_eq!(
            classify_timescale(Some("2.31.9"), Some("timescale")),
            TimescaleSupport::TooOld("2.31.9".to_owned())
        );
        assert_eq!(
            classify_timescale(Some("2.32.0"), Some("apache")),
            TimescaleSupport::Unlicensed(Some("apache".to_owned()))
        );
        assert_eq!(
            classify_timescale(Some("2.32.0"), None),
            TimescaleSupport::Unlicensed(None)
        );
        assert_eq!(classify_timescale(None, None), TimescaleSupport::Missing);
    }

    #[test]
    fn only_a_blank_schema_may_fresh_bootstrap() {
        assert_eq!(
            decide_schema(&evidence(), false),
            Ok(SchemaAdmission::FreshBootstrap)
        );
        let mut partial = evidence();
        partial.has_migrations_table = true;
        assert!(matches!(
            decide_schema(&partial, false),
            Err(SchemaRefusal::PartialApplicationSchema { .. })
        ));
        let mut gated = evidence();
        gated.has_gate_table = true;
        assert!(matches!(
            decide_schema(&gated, true),
            Err(SchemaRefusal::PartialApplicationSchema { .. })
        ));
    }

    #[test]
    fn legacy_request_relations_refuse_with_or_without_a_gate() {
        let mut legacy = evidence();
        legacy.has_migrations_table = true;
        legacy.request_relations = vec!["request_records".to_owned()];
        assert!(matches!(
            decide_schema(&legacy, false),
            Err(SchemaRefusal::LegacyRequestSchema { .. })
        ));
        legacy.has_gate_table = true;
        assert!(matches!(
            decide_schema(&legacy, true),
            Err(SchemaRefusal::LegacyRequestSchema { .. })
        ));
    }

    #[test]
    fn a_complete_valid_layout_is_accepted_for_join_and_upgrade() {
        let mut complete = evidence();
        complete.has_migrations_table = true;
        complete.has_gate_table = true;
        complete.request_relations = REQUEST_HYPERTABLES
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        complete.hypertables = hypertables();
        assert_eq!(
            decide_schema(&complete, true),
            Ok(SchemaAdmission::Existing)
        );

        complete.hypertables[0].daily = false;
        assert!(matches!(
            decide_schema(&complete, true),
            Err(SchemaRefusal::LegacyRequestSchema { .. })
        ));
        complete.hypertables = hypertables();
        let mut extra = complete.request_relations.clone();
        extra.push("request_records_20260101".to_owned());
        complete.request_relations = extra;
        assert!(matches!(
            decide_schema(&complete, true),
            Err(SchemaRefusal::LegacyRequestSchema { .. })
        ));
    }

    #[test]
    fn unsupported_timescale_refuses_before_any_schema_decision() {
        for (version, license) in [
            (None, None),
            (Some("2.31.0"), Some("timescale")),
            (Some("2.32.0"), Some("apache")),
        ] {
            let mut evidence = evidence();
            evidence.timescale_version = version.map(str::to_owned);
            evidence.timescale_license = license.map(str::to_owned);
            assert!(matches!(
                decide_schema(&evidence, false),
                Err(SchemaRefusal::Timescale(_))
            ));
        }
    }

    #[test]
    fn migration_decision_matrix() {
        let shipped = [embedded(1, b"a"), embedded(2, b"b"), embedded(3, b"c")];

        // Join requires the exact shipped set.
        assert_eq!(
            decide_migrations(
                GuardMode::Join,
                &[applied(1, b"a", true), applied(2, b"b", true)],
                &shipped
            ),
            Err(MigrationRefusal::PendingOnJoin { versions: vec![3] })
        );
        assert_eq!(
            decide_migrations(
                GuardMode::Join,
                &[
                    applied(1, b"a", true),
                    applied(2, b"b", true),
                    applied(3, b"c", true)
                ],
                &shipped
            ),
            Ok(())
        );
        // An upgrade may start from a strict prefix.
        assert_eq!(
            decide_migrations(
                GuardMode::Upgrade,
                &[applied(1, b"a", true), applied(2, b"b", true)],
                &shipped
            ),
            Ok(())
        );
        assert_eq!(
            decide_migrations(GuardMode::Bootstrap, &[applied(1, b"a", true)], &shipped),
            Ok(())
        );
        // A gap after the prefix is not a valid pending state.
        assert_eq!(
            decide_migrations(
                GuardMode::Upgrade,
                &[applied(1, b"a", true), applied(3, b"c", true)],
                &shipped
            ),
            Err(MigrationRefusal::MissingEmbedded { version: 2 })
        );
        assert_eq!(
            decide_migrations(
                GuardMode::Upgrade,
                &[applied(1, b"a", true)],
                &[embedded(1, b"a"), embedded(3, b"c")]
            ),
            Ok(())
        );
        // Dirty, mismatch and unknown applied rows refuse on every path.
        for mode in [GuardMode::Join, GuardMode::Upgrade, GuardMode::Bootstrap] {
            assert_eq!(
                decide_migrations(mode, &[applied(1, b"a", false)], &shipped),
                Err(MigrationRefusal::Dirty { version: 1 })
            );
            assert_eq!(
                decide_migrations(mode, &[applied(1, b"x", true)], &shipped),
                Err(MigrationRefusal::ChecksumMismatch { version: 1 })
            );
            assert_eq!(
                decide_migrations(mode, &[applied(9, b"x", true)], &shipped),
                Err(MigrationRefusal::UnknownApplied { version: 9 })
            );
        }
    }
}
