//! Administrator configuration import: dry-run preview and atomic replace.
//!
//! The import side of the encrypted configuration archive. `preview` decodes
//! and validates the archive against the running backend and reports
//! per-domain differences without writing anything; `apply` repeats the
//! validation and then replaces the restorable domains inside a single
//! backend transaction so a failure leaves no half-restored instance.
//!
//! Nothing here reads request/usage/billing/session data, and no
//! secret, passphrase, or archive byte is ever logged, echoed, or placed in a
//! URL. The passphrase exists only inside the request scope.
//!
//! Backend readers/writers live in [`postgres`] and [`sqlite`]; this module
//! owns the portable types, validation, and preview diff.

mod diff;
mod postgres;
mod postgres_domains;
mod sqlite;
mod sqlite_rows;

use serde::Serialize;
use utoipa::ToSchema;

use super::archive::{ArchiveError, decode_archive_for_backend};
use super::snapshot::{ConfigBackendKind, ConfigSnapshot, SNAPSHOT_FORMAT_VERSION};
use super::{ConfigRepository, build_config_snapshot};
use crate::db::UserStore;
use crate::relay_secrets::RelaySecretManager;

/// The restorable domains of a current (format 3) archive, in manifest order.
/// Used to reject an archive whose manifest declares an unknown or missing
/// domain. A format-2 archive is normalized by the archive decoder before the
/// importer sees it.
const DOMAIN_NAMES: [&str; 10] = [
    "users",
    "client_keys",
    "endpoints",
    "model_routes",
    "mcp_servers",
    "mcp_credentials",
    "settings",
    "user_redaction_configs",
    "mcp_oauth",
    "user_endpoint_settings",
];

/// The exact native format-3 domain set written before explicit per-user
/// endpoint preferences became portable. Frozen: those archives remain
/// importable with the preference domain empty.
const LEGACY_V3_DOMAIN_NAMES: [&str; 9] = [
    "users",
    "client_keys",
    "endpoints",
    "model_routes",
    "mcp_servers",
    "mcp_credentials",
    "settings",
    "user_redaction_configs",
    "mcp_oauth",
];

/// Stable error surface for the import path. `code()` never carries archive
/// bytes, secrets, or the passphrase.
#[derive(Debug)]
pub enum ImportError {
    Archive(ArchiveError),
    /// The archive targets a domain the destination backend cannot store.
    UnsupportedDomain {
        domain: &'static str,
        backend: ConfigBackendKind,
    },
    /// The decoded payload is structurally unusable for import.
    InvalidArchive(&'static str),
    /// The request body or encoded archive exceeds the accepted size.
    TooLarge {
        limit: usize,
    },
    /// A destination write failed; the backend transaction is rolled back.
    Write(String),
}

impl ImportError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Archive(err) => err.code(),
            Self::UnsupportedDomain { .. } => "config_import_unsupported_domain",
            Self::InvalidArchive(_) => "config_import_invalid_archive",
            Self::TooLarge { .. } => "config_import_too_large",
            Self::Write(_) => "config_import_write_failed",
        }
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(err) => write!(formatter, "{err}"),
            Self::UnsupportedDomain { domain, backend } => write!(
                formatter,
                "the {backend} backend cannot store the {domain} configuration domain"
            ),
            Self::InvalidArchive(reason) => {
                write!(
                    formatter,
                    "configuration archive cannot be imported: {reason}"
                )
            }
            Self::TooLarge { limit } => {
                write!(
                    formatter,
                    "configuration archive import request exceeds the {limit} byte limit"
                )
            }
            Self::Write(reason) => write!(formatter, "configuration import failed: {reason}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<ArchiveError> for ImportError {
    fn from(err: ArchiveError) -> Self {
        Self::Archive(err)
    }
}

/// Per-domain dry-run difference. Counts and identifiers only; never payloads.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportDomainDiff {
    pub name: String,
    pub archive_records: usize,
    pub target_records: usize,
    pub creates: usize,
    pub updates: usize,
    pub deletes: usize,
    pub unrecoverable_secrets: usize,
}

/// Non-secret preview of what an import would change.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportPreview {
    #[schema(value_type = String, example = "sqlite")]
    pub backend_kind: ConfigBackendKind,
    pub format_version: u16,
    #[schema(value_type = String)]
    pub exported_at: chrono::DateTime<chrono::Utc>,
    pub payload_fingerprint: String,
    pub domains: Vec<ConfigImportDomainDiff>,
    pub warnings: Vec<String>,
}

/// One domain restored by a committed import.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportDomainSummary {
    pub name: String,
    pub records: usize,
    pub unrecoverable_secrets: usize,
}

/// Result of a committed import.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConfigImportApplied {
    #[schema(value_type = String, example = "sqlite")]
    pub backend_kind: ConfigBackendKind,
    pub format_version: u16,
    pub payload_fingerprint: String,
    pub domains: Vec<ConfigImportDomainSummary>,
}

/// Decode and validate the archive against the running backend. Read-only.
pub fn decode_for_repository(
    repository: &ConfigRepository,
    passphrase: &str,
    archive_bytes: &[u8],
) -> Result<ConfigSnapshot, ImportError> {
    let expected = if repository.is_sqlite() {
        ConfigBackendKind::Sqlite
    } else {
        ConfigBackendKind::Postgres
    };
    let snapshot = decode_archive_for_backend(passphrase, archive_bytes, Some(expected))?;
    validate_snapshot(&snapshot, expected)?;
    Ok(snapshot)
}

/// Reject archives whose manifest declares a shape the importer does not know,
/// or whose domains the destination backend cannot store.
fn validate_snapshot(
    snapshot: &ConfigSnapshot,
    backend: ConfigBackendKind,
) -> Result<(), ImportError> {
    if snapshot.manifest.format_version != SNAPSHOT_FORMAT_VERSION {
        return Err(ImportError::InvalidArchive("unsupported format version"));
    }
    let names: Vec<&str> = snapshot
        .manifest
        .domains
        .iter()
        .map(|domain| domain.name.as_str())
        .collect();
    let declared: std::collections::BTreeSet<&str> = names.iter().copied().collect();
    if declared.len() != names.len() {
        return Err(ImportError::InvalidArchive("duplicate snapshot domain"));
    }
    let current = declared == DOMAIN_NAMES.into_iter().collect();
    let legacy_v3 = declared == LEGACY_V3_DOMAIN_NAMES.into_iter().collect();
    if !current && !legacy_v3 {
        return Err(ImportError::InvalidArchive("unexpected snapshot domains"));
    }
    if legacy_v3 && !snapshot.domains.user_endpoint_settings.is_empty() {
        return Err(ImportError::InvalidArchive(
            "an old archive declares no user_endpoint_settings domain but carries preferences",
        ));
    }
    // The raw object-store configuration is deployment state, never portable
    // configuration. A hand-built format-3 archive must not smuggle it through
    // the settings domain.
    if snapshot
        .domains
        .settings
        .iter()
        .any(|setting| setting.key == super::snapshot::legacy::LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY)
    {
        return Err(ImportError::InvalidArchive(
            "the archive carries the raw object store setting",
        ));
    }
    if backend == ConfigBackendKind::Sqlite {
        // SQLite stores neither per-user redaction rules nor personal MCP
        // OAuth grants; a payload carrying them is a foreign shape and must be
        // rejected instead of silently dropped.
        if !snapshot.domains.user_redaction_configs.is_empty() {
            return Err(ImportError::UnsupportedDomain {
                domain: "user_redaction_configs",
                backend,
            });
        }
        if !snapshot.domains.mcp_oauth.is_empty() {
            return Err(ImportError::UnsupportedDomain {
                domain: "mcp_oauth",
                backend,
            });
        }
        if !snapshot.domains.user_endpoint_settings.is_empty() {
            return Err(ImportError::UnsupportedDomain {
                domain: "user_endpoint_settings",
                backend,
            });
        }
    }
    validate_grant_parents(snapshot)?;
    validate_user_endpoint_preferences(snapshot)?;
    validate_endpoint_api_key_ids(snapshot)?;
    Ok(())
}

/// Reject an archive whose endpoint API keys reuse one stored identifier.
///
/// The destination inserts every stored key with its archived primary key, so
/// a duplicate identifier would fail mid-import after the destructive delete.
/// The check is global across endpoints and keeps a real stored nil UUID:
/// only repetition is invalid, not the value itself.
fn validate_endpoint_api_key_ids(snapshot: &ConfigSnapshot) -> Result<(), ImportError> {
    let mut seen = std::collections::BTreeSet::new();
    for endpoint in &snapshot.domains.endpoints {
        for key in &endpoint.api_keys {
            if !seen.insert(key.key_id) {
                return Err(ImportError::InvalidArchive(
                    "duplicate endpoint API key identifier",
                ));
            }
        }
    }
    Ok(())
}

/// Reject an archive whose personal MCP OAuth grants have no parent.
///
/// A grant without its server or its owner cannot be restored: importing it
/// would either fail on the foreign key or attach a personal credential to
/// the wrong account. The archive is refused before any destructive write.
fn validate_grant_parents(snapshot: &ConfigSnapshot) -> Result<(), ImportError> {
    for grant in &snapshot.domains.mcp_oauth {
        let Some(server) = snapshot
            .domains
            .mcp_servers
            .iter()
            .find(|server| server.server_id == grant.server_id)
        else {
            return Err(ImportError::InvalidArchive(
                "a personal MCP OAuth grant has no server in the archive",
            ));
        };
        if server.owner_user_id != Some(grant.owner_user_id) {
            return Err(ImportError::InvalidArchive(
                "a personal MCP OAuth grant does not belong to the archived server owner",
            ));
        }
        if !snapshot
            .domains
            .users
            .iter()
            .any(|user| user.user_id == grant.owner_user_id)
        {
            return Err(ImportError::InvalidArchive(
                "a personal MCP OAuth grant has no owner in the archive",
            ));
        }
    }
    Ok(())
}

/// Reject an archive whose explicit per-user endpoint preference cannot be
/// restored. The user and any selected endpoint must exist in the same archive,
/// and one user cannot carry two preference rows. Duplicate, unknown or parent
/// mismatches fail before any destructive write; the write path itself accepts
/// disabled/private endpoints because the stored preference is authoritative.
fn validate_user_endpoint_preferences(snapshot: &ConfigSnapshot) -> Result<(), ImportError> {
    let mut seen = std::collections::BTreeSet::new();
    for setting in &snapshot.domains.user_endpoint_settings {
        if !seen.insert(setting.user_id) {
            return Err(ImportError::InvalidArchive(
                "duplicate user_endpoint_settings preference",
            ));
        }
        if !snapshot
            .domains
            .users
            .iter()
            .any(|user| user.user_id == setting.user_id)
        {
            return Err(ImportError::InvalidArchive(
                "a user_endpoint_settings row has no user in the archive",
            ));
        }
        if let Some(endpoint_id) = setting.endpoint_id
            && !snapshot
                .domains
                .endpoints
                .iter()
                .any(|endpoint| endpoint.endpoint_id == endpoint_id)
        {
            return Err(ImportError::InvalidArchive(
                "a user_endpoint_settings row selects an endpoint absent from the archive",
            ));
        }
    }
    Ok(())
}

/// Read-only dry run: decode the archive, compare it with the running
/// configuration, and return the per-domain differences plus warnings.
pub async fn preview_config_import(
    repository: &ConfigRepository,
    users: &UserStore,
    manager: Option<&RelaySecretManager>,
    passphrase: &str,
    archive_bytes: &[u8],
) -> Result<ConfigImportPreview, ImportError> {
    let archive = decode_for_repository(repository, passphrase, archive_bytes)?;
    let target = build_config_snapshot(repository, users, manager)
        .await
        .map_err(|err| {
            ImportError::Write(format!("failed to read the current configuration: {err}"))
        })?;
    let domains = diff::diff_domains(&archive.domains, &target.domains);
    let warnings = diff::import_warnings(&archive);
    Ok(ConfigImportPreview {
        backend_kind: archive.manifest.backend_kind,
        format_version: archive.manifest.format_version,
        exported_at: archive.manifest.exported_at,
        payload_fingerprint: archive.manifest.payload_fingerprint.clone(),
        domains,
        warnings,
    })
}

/// Replace the restorable configuration domains with the archive contents in
/// one backend transaction.
pub async fn apply_config_import(
    repository: &ConfigRepository,
    manager: Option<&RelaySecretManager>,
    passphrase: &str,
    archive_bytes: &[u8],
) -> Result<ConfigImportApplied, ImportError> {
    let archive = decode_for_repository(repository, passphrase, archive_bytes)?;
    match repository {
        ConfigRepository::Postgres(repo) => {
            let import_manager = manager.ok_or(ImportError::InvalidArchive(
                "the worker has no configuration encryption key for relay secrets",
            ))?;
            postgres::apply(repo.pool(), import_manager, &archive.domains)
                .await
                .map_err(|err| ImportError::Write(format!("{err:#}")))?;
        }
        ConfigRepository::Sqlite(repo) => {
            sqlite::apply(repo.store(), repo.manager(), &archive.domains)
                .await
                .map_err(|err| ImportError::Write(format!("{err:#}")))?;
        }
    }
    Ok(ConfigImportApplied {
        backend_kind: archive.manifest.backend_kind,
        format_version: archive.manifest.format_version,
        payload_fingerprint: archive.manifest.payload_fingerprint.clone(),
        domains: archive
            .manifest
            .domains
            .iter()
            .map(|domain| ConfigImportDomainSummary {
                name: domain.name.clone(),
                records: domain.records,
                unrecoverable_secrets: domain.unrecoverable_secrets,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::config_repository::archive_test_fixtures::GRANT_OWNER_USER_ID;
    use crate::db::config_repository::snapshot::{
        ConfigSnapshot, McpServerSnapshot, SnapshotDomainSummary, SnapshotDomains,
    };

    fn manifest_with(names: &[&str]) -> Vec<SnapshotDomainSummary> {
        names
            .iter()
            .map(|name| SnapshotDomainSummary {
                name: (*name).to_string(),
                records: 0,
                unrecoverable_secrets: 0,
            })
            .collect()
    }

    fn snapshot_with_domains(domains: Vec<SnapshotDomainSummary>) -> ConfigSnapshot {
        let mut snapshot = ConfigSnapshot::new(
            ConfigBackendKind::Postgres,
            SnapshotDomains {
                users: Vec::new(),
                client_keys: Vec::new(),
                endpoints: Vec::new(),
                model_routes: Vec::new(),
                mcp_servers: Vec::new(),
                mcp_credentials: Vec::new(),
                settings: Vec::new(),
                user_redaction_configs: Vec::new(),
                mcp_oauth: Vec::new(),
                user_endpoint_settings: Vec::new(),
            },
        );
        snapshot.manifest.domains = domains;
        snapshot
    }

    fn server(owner_user_id: i64) -> McpServerSnapshot {
        let stamp = chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("timestamp");
        McpServerSnapshot {
            server_id: uuid::Uuid::parse_str(super::super::archive_test_fixtures::GRANT_SERVER_ID)
                .expect("server id"),
            source_endpoint_id: None,
            scope: "user".to_string(),
            owner_user_id: Some(owner_user_id),
            name: "archived".to_string(),
            aggregate_naming_mode: "disabled".to_string(),
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("https://mcp.example.invalid".to_string()),
            command: None,
            args: serde_json::Value::Null,
            env_json: serde_json::Value::Null,
            http_headers_json: serde_json::Value::Null,
            auth_mode: "oauth".to_string(),
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            tool_filter_mode: "disabled".to_string(),
            allowed_tools: serde_json::Value::Null,
            disabled_tools: serde_json::Value::Null,
            disabled_resources: serde_json::Value::Null,
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "manual".to_string(),
            lifecycle_manual_protocol_version: None,
            created_at: stamp,
            updated_at: stamp,
        }
    }

    fn grant() -> crate::db::McpOAuthGrantSnapshot {
        super::super::archive_test_fixtures::sample_grant()
    }

    #[test]
    fn a_current_domain_set_is_accepted_and_a_normalized_legacy_one_too() {
        let current = snapshot_with_domains(manifest_with(&DOMAIN_NAMES));
        validate_snapshot(&current, ConfigBackendKind::Postgres).expect("current archive accepted");

        let normalized = crate::db::config_repository::snapshot::legacy::normalize(
            crate::db::config_repository::archive_test_fixtures::LEGACY_PAYLOAD_JSON.as_bytes(),
        )
        .expect("legacy payload normalizes");
        validate_snapshot(&normalized, ConfigBackendKind::Postgres)
            .expect("a normalized legacy archive is importable");
    }

    #[test]
    fn an_unknown_duplicate_or_missing_domain_is_rejected() {
        let mut unknown = DOMAIN_NAMES.to_vec();
        unknown.push("raw_payloads");
        let snapshot = snapshot_with_domains(manifest_with(&unknown));
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));

        let mut duplicate = DOMAIN_NAMES.to_vec();
        duplicate.push("mcp_oauth");
        let snapshot = snapshot_with_domains(manifest_with(&duplicate));
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));

        let mut missing = DOMAIN_NAMES.to_vec();
        missing.retain(|name| *name != "settings");
        let snapshot = snapshot_with_domains(manifest_with(&missing));
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));
    }

    #[test]
    fn a_raw_object_store_setting_cannot_be_smuggled_through_settings() {
        let mut snapshot = snapshot_with_domains(manifest_with(&DOMAIN_NAMES));
        snapshot.domains.settings = vec![crate::db::config_repository::snapshot::SettingSnapshot {
            key:
                crate::db::config_repository::snapshot::legacy::LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY
                    .to_string(),
            version: 1,
            value: serde_json::Value::Null,
            updated_at: None,
        }];
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(
                "the archive carries the raw object store setting"
            ))
        ));
    }

    #[test]
    fn a_grant_without_its_server_or_owner_is_rejected() {
        let mut snapshot = snapshot_with_domains(manifest_with(&DOMAIN_NAMES));
        snapshot.domains.mcp_oauth = vec![grant()];
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));

        let mut snapshot = snapshot_with_domains(manifest_with(&DOMAIN_NAMES));
        snapshot.domains.mcp_servers = vec![server(42)];
        snapshot.domains.mcp_oauth = vec![grant()];
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));
    }

    #[test]
    fn sqlite_cannot_store_a_grant() {
        let mut snapshot = snapshot_with_domains(manifest_with(&DOMAIN_NAMES));
        snapshot.domains.mcp_servers = vec![server(GRANT_OWNER_USER_ID)];
        snapshot.domains.mcp_oauth = vec![grant()];
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Sqlite),
            Err(ImportError::UnsupportedDomain {
                domain: "mcp_oauth",
                ..
            })
        ));
    }
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    use crate::db::config_repository::snapshot::{ConfigSnapshot, UserEndpointSettingSnapshot};

    fn old_nine() -> ConfigSnapshot {
        serde_json::from_slice(
            crate::db::config_repository::archive_test_fixtures::V3_NINE_PAYLOAD_JSON.as_bytes(),
        )
        .expect("frozen old V3 payload decodes")
    }

    fn current_with_pref(user_id: i64, endpoint_id: Option<uuid::Uuid>) -> ConfigSnapshot {
        let mut snapshot = old_nine();
        snapshot.manifest.domains.push(
            crate::db::config_repository::snapshot::SnapshotDomainSummary {
                name: "user_endpoint_settings".to_string(),
                records: 1,
                unrecoverable_secrets: 0,
            },
        );
        snapshot.domains.user_endpoint_settings = vec![UserEndpointSettingSnapshot {
            user_id,
            endpoint_id,
        }];
        snapshot
    }

    #[test]
    fn the_frozen_old_nine_domain_shape_is_accepted_with_empty_preferences() {
        validate_snapshot(&old_nine(), ConfigBackendKind::Postgres)
            .expect("the old native V3 shape remains importable");
    }

    #[test]
    fn an_old_nine_archive_cannot_smuggle_undeclared_preferences() {
        let mut snapshot = old_nine();
        snapshot.domains.user_endpoint_settings = vec![UserEndpointSettingSnapshot {
            user_id: 1,
            endpoint_id: None,
        }];
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(
                "an old archive declares no user_endpoint_settings domain but carries preferences"
            ))
        ));
    }

    #[test]
    fn a_preference_parent_must_exist_and_be_unique() {
        validate_snapshot(&current_with_pref(1, None), ConfigBackendKind::Postgres)
            .expect("a NULL preference keeps the routing default");

        let mut unknown_user = current_with_pref(99, None);
        assert!(matches!(
            validate_snapshot(&unknown_user, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));

        unknown_user.domains.user_endpoint_settings = vec![UserEndpointSettingSnapshot {
            user_id: 1,
            endpoint_id: Some(uuid::Uuid::new_v4()),
        }];
        assert!(matches!(
            validate_snapshot(&unknown_user, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));

        let mut duplicate = current_with_pref(1, None);
        duplicate.domains.user_endpoint_settings = vec![
            UserEndpointSettingSnapshot {
                user_id: 1,
                endpoint_id: None,
            },
            UserEndpointSettingSnapshot {
                user_id: 1,
                endpoint_id: None,
            },
        ];
        assert!(matches!(
            validate_snapshot(&duplicate, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(_))
        ));
    }

    #[test]
    fn sqlite_rejects_nonempty_preferences_like_other_postgres_only_domains() {
        assert!(matches!(
            validate_snapshot(&current_with_pref(1, None), ConfigBackendKind::Sqlite),
            Err(ImportError::UnsupportedDomain {
                domain: "user_endpoint_settings",
                ..
            })
        ));
    }
}

#[cfg(test)]
mod endpoint_key_tests {
    use super::*;
    use crate::db::config_repository::snapshot::EndpointApiKeySnapshot;

    fn old_nine() -> ConfigSnapshot {
        serde_json::from_slice(
            crate::db::config_repository::archive_test_fixtures::V3_NINE_PAYLOAD_JSON.as_bytes(),
        )
        .expect("frozen old V3 payload decodes")
    }

    fn endpoint() -> crate::db::config_repository::snapshot::EndpointSnapshot {
        serde_json::from_value(serde_json::json!({
            "endpoint_id": "388fd214-afba-7cb4-c32d-02a2858cdec4",
            "scope": "admin",
            "owner_user_id": null,
            "name": "ep-one",
            "provider": "generic",
            "provider_region": null,
            "service_tier": null,
            "base_url": "http://127.0.0.1:1",
            "native_api": "chat",
            "native_api_source": "manual",
            "key_lb_enabled": false,
            "enabled": true,
            "mcp_enabled": false,
            "active_windows": null,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z",
            "api_key": "primary",
            "api_keys": [],
            "proxy_url": null,
            "admin_api_key": null,
            "oauth": null
        }))
        .expect("endpoint fixture")
    }

    #[test]
    fn duplicate_persisted_endpoint_key_ids_are_rejected_across_endpoints() {
        let mut snapshot = old_nine();
        snapshot.domains.endpoints.push(endpoint());
        snapshot.domains.endpoints[0]
            .api_keys
            .push(EndpointApiKeySnapshot {
                key_id: uuid::Uuid::from_u64_pair(7, 0),
                key_label: "stored".to_string(),
                position: 0,
                enabled: true,
                api_key: Some("stored-secret".to_string()),
            });
        let mut copy = snapshot.domains.endpoints[0].clone();
        copy.endpoint_id = uuid::Uuid::new_v4();
        copy.name = "copied-endpoint".to_string();
        snapshot.domains.endpoints.push(copy);
        assert!(matches!(
            validate_snapshot(&snapshot, ConfigBackendKind::Postgres),
            Err(ImportError::InvalidArchive(
                "duplicate endpoint API key identifier"
            ))
        ));
    }

    #[test]
    fn one_persisted_nil_key_with_legacy_only_endpoints_is_accepted() {
        let mut snapshot = old_nine();
        snapshot.domains.endpoints.push(endpoint());
        for endpoint in &mut snapshot.domains.endpoints {
            endpoint.api_keys.clear();
        }
        snapshot.domains.endpoints[0]
            .api_keys
            .push(EndpointApiKeySnapshot {
                key_id: uuid::Uuid::nil(),
                key_label: "legacy-nil".to_string(),
                position: 3,
                enabled: false,
                api_key: Some("stored-secret".to_string()),
            });
        validate_snapshot(&snapshot, ConfigBackendKind::Postgres)
            .expect("a single real stored nil key is importable");
    }
}
