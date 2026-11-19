//! Bounded compatibility reader for the format-2 configuration payload.
//!
//! A format-2 payload is decoded only to migrate it: the frozen record shapes
//! below reproduce the exact field order and serialization of that payload
//! format, so the original fingerprint can be verified before anything is
//! discarded. The verified payload is then normalized to the current format-3
//! domains, dropping deployment-only data (the raw object-store configuration,
//! coordination settings) and the retired MCP credential money fields.
//!
//! This module never writes a format-2 archive: [`super::manifest::ConfigSnapshot`]
//! is the only encodable shape, and it always seals as format 3.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::domains::{
    ClientKeySnapshot, EndpointSnapshot, McpCredentialSnapshot, McpServerSnapshot,
    ModelRouteSnapshot, SettingSnapshot, SnapshotDomains, UserRedactionConfigSnapshot,
    UserSnapshot,
};
use super::manifest::{
    ConfigSnapshot, SNAPSHOT_FORMAT_VERSION, SnapshotDiscarded, SnapshotManifest,
    SnapshotProvenance, fingerprint_of,
};
use crate::db::{
    INTERNAL_COORDINATION_SETTING_KEYS, McpOAuthGrantSnapshot, SHARED_VALKEY_SETTINGS_KEY,
};

/// Payload format this reader understands.
pub(crate) const LEGACY_FORMAT_VERSION: u16 = 2;

/// Deployment-state setting that only the format-2 reader and the import
/// guard recognize. The runtime has no raw object-store setting any more.
pub(crate) const LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY: &str = "raw_object_store";

/// The domain set of a format-2 payload written before the personal MCP OAuth
/// domain existed.
const PRE_GRANT_DOMAIN_NAMES: [&str; 9] = [
    "users",
    "client_keys",
    "endpoints",
    "model_routes",
    "mcp_servers",
    "mcp_credentials",
    "settings",
    "user_redaction_configs",
    "raw_object_store",
];

/// The domain set of a format-2 payload written after that domain existed.
const POST_GRANT_DOMAIN_NAMES: [&str; 10] = [
    "users",
    "client_keys",
    "endpoints",
    "model_routes",
    "mcp_servers",
    "mcp_credentials",
    "settings",
    "user_redaction_configs",
    "raw_object_store",
    "mcp_oauth",
];

/// The exact format-2 credential record, including the retired cost fields that
/// used to be serialized in this order.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyV2McpCredentialSnapshot {
    credential_id: Uuid,
    server_id: Uuid,
    credential_label: String,
    secret: String,
    position: i32,
    enabled: bool,
    provider_kind: Option<String>,
    default_cost: f64,
    strict_mode: bool,
    billing_period_start: Option<DateTime<Utc>>,
    billing_period_end: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// The exact format-2 domain records. The field order is the serialized order
/// the original fingerprint covers.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyV2Domains {
    users: Vec<UserSnapshot>,
    client_keys: Vec<ClientKeySnapshot>,
    endpoints: Vec<EndpointSnapshot>,
    model_routes: Vec<ModelRouteSnapshot>,
    mcp_servers: Vec<McpServerSnapshot>,
    mcp_credentials: Vec<LegacyV2McpCredentialSnapshot>,
    settings: Vec<SettingSnapshot>,
    user_redaction_configs: Vec<UserRedactionConfigSnapshot>,
    raw_object_store: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mcp_oauth: Vec<McpOAuthGrantSnapshot>,
}

/// A format-2 payload. Only deserialization is needed: it is never written.
#[derive(Debug, Deserialize)]
struct LegacyV2Snapshot {
    manifest: SnapshotManifest,
    #[serde(flatten)]
    domains: LegacyV2Domains,
}

/// Parse, verify, and normalize one format-2 payload.
///
/// The original domain names and fingerprint are checked before the payload is
/// reshaped, so a tampered or unknown legacy payload is rejected rather than
/// silently normalized.
pub(crate) fn normalize(plaintext: &[u8]) -> Result<ConfigSnapshot, &'static str> {
    let legacy: LegacyV2Snapshot =
        serde_json::from_slice(plaintext).map_err(|_| "unreadable snapshot")?;
    if legacy.manifest.format_version != LEGACY_FORMAT_VERSION {
        return Err("unexpected payload version for the legacy reader");
    }
    validate_domain_names(&legacy.manifest)?;
    if !declares_personal_grants(&legacy.manifest.domains) && !legacy.domains.mcp_oauth.is_empty() {
        return Err("a format-2 payload without the grant domain carries grants");
    }
    let original =
        fingerprint_of(&legacy.domains).map_err(|_| "unreadable legacy snapshot payload")?;
    if original != legacy.manifest.payload_fingerprint {
        return Err("legacy snapshot payload fingerprint mismatch");
    }
    normalize_verified(legacy)
}

fn validate_domain_names(manifest: &SnapshotManifest) -> Result<(), &'static str> {
    let names: Vec<&str> = manifest
        .domains
        .iter()
        .map(|domain| domain.name.as_str())
        .collect();
    let declared: std::collections::BTreeSet<&str> = names.iter().copied().collect();
    if declared.len() != names.len() {
        return Err("duplicate snapshot domain");
    }
    let pre_grant = declared == PRE_GRANT_DOMAIN_NAMES.into_iter().collect();
    let post_grant = declared == POST_GRANT_DOMAIN_NAMES.into_iter().collect();
    if !pre_grant && !post_grant {
        return Err("unexpected snapshot domains");
    }
    Ok(())
}

fn declares_personal_grants(domains: &[super::manifest::SnapshotDomainSummary]) -> bool {
    domains.iter().any(|domain| domain.name == "mcp_oauth")
}

/// Reshape a verified format-2 payload into the current domain set.
fn normalize_verified(legacy: LegacyV2Snapshot) -> Result<ConfigSnapshot, &'static str> {
    let LegacyV2Snapshot { manifest, domains } = legacy;
    let mut discarded = Vec::new();

    if domains.raw_object_store.is_some() {
        discarded.push(SnapshotDiscarded {
            name: "raw_object_store".to_string(),
            records: 1,
        });
    }

    let mut settings = Vec::with_capacity(domains.settings.len());
    let mut raw_settings = 0usize;
    let mut coordination_settings = 0usize;
    for setting in domains.settings {
        if setting.key == LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY {
            raw_settings += 1;
            continue;
        }
        if setting.key == SHARED_VALKEY_SETTINGS_KEY
            || INTERNAL_COORDINATION_SETTING_KEYS.contains(&setting.key.as_str())
        {
            coordination_settings += 1;
            continue;
        }
        settings.push(setting);
    }
    if raw_settings > 0 {
        discarded.push(SnapshotDiscarded {
            name: "settings.raw_object_store".to_string(),
            records: raw_settings,
        });
    }
    if coordination_settings > 0 {
        discarded.push(SnapshotDiscarded {
            name: "settings.coordination".to_string(),
            records: coordination_settings,
        });
    }

    let mut retired_fields = 0usize;
    let mcp_credentials = domains
        .mcp_credentials
        .into_iter()
        .map(|credential| {
            if credential.default_cost != 1.0
                || credential.strict_mode
                || credential.billing_period_start.is_some()
                || credential.billing_period_end.is_some()
            {
                retired_fields += 1;
            }
            McpCredentialSnapshot {
                credential_id: credential.credential_id,
                server_id: credential.server_id,
                credential_label: credential.credential_label,
                secret: credential.secret,
                position: credential.position,
                enabled: credential.enabled,
                provider_kind: credential.provider_kind,
                created_at: credential.created_at,
                updated_at: credential.updated_at,
            }
        })
        .collect();
    if retired_fields > 0 {
        discarded.push(SnapshotDiscarded {
            name: "mcp_credentials.retired_fields".to_string(),
            records: retired_fields,
        });
    }

    let domains = SnapshotDomains {
        users: domains.users,
        client_keys: domains.client_keys,
        endpoints: domains.endpoints,
        model_routes: domains.model_routes,
        mcp_servers: domains.mcp_servers,
        mcp_credentials,
        settings,
        user_redaction_configs: domains.user_redaction_configs,
        mcp_oauth: domains.mcp_oauth,
        user_endpoint_settings: Vec::new(),
    };
    let mut missing_domains = vec!["user_endpoint_settings".to_string()];
    if !declares_personal_grants(&manifest.domains) {
        missing_domains.push("mcp_oauth".to_string());
    }
    let mut snapshot = ConfigSnapshot {
        manifest: SnapshotManifest {
            format_version: SNAPSHOT_FORMAT_VERSION,
            backend_kind: manifest.backend_kind,
            exported_at: manifest.exported_at,
            payload_fingerprint: String::new(),
            domains: Vec::new(),
            provenance: Some(SnapshotProvenance {
                source_format_version: LEGACY_FORMAT_VERSION,
                source_payload_fingerprint: manifest.payload_fingerprint,
                missing_domains,
                discarded,
            }),
        },
        domains,
    };
    snapshot
        .seal()
        .map_err(|_| "failed to normalize the legacy snapshot")?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::config_repository::archive_test_fixtures::{
        LEGACY_DOMAINS_JSON, LEGACY_FINGERPRINT, LEGACY_MONEY_DOMAINS_JSON,
        LEGACY_MONEY_FINGERPRINT, LEGACY_MONEY_PAYLOAD_JSON, LEGACY_PAYLOAD_JSON,
    };

    #[test]
    fn a_pinned_pre_grant_payload_keeps_its_bytes_and_normalizes() {
        let normalized = normalize(LEGACY_PAYLOAD_JSON.as_bytes()).expect("legacy payload");
        let provenance = normalized.manifest.provenance.as_ref().expect("provenance");
        assert_eq!(provenance.source_format_version, LEGACY_FORMAT_VERSION);
        assert_eq!(provenance.source_payload_fingerprint, LEGACY_FINGERPRINT);
        assert_eq!(normalized.manifest.format_version, SNAPSHOT_FORMAT_VERSION);
        assert_eq!(normalized.domains.users.len(), 1);
        assert!(normalized.domains.mcp_oauth.is_empty());
        assert_eq!(
            serde_json::to_string(&normalized.domains).expect("normalized domains"),
            r#"{"users":[{"user_id":1,"login_name":"admin","display_name":"admin","password_hash":"$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA","is_admin":true,"is_active":true,"created_at":"2023-11-14T22:13:20Z","updated_at":"2023-11-14T22:13:20Z"}],"client_keys":[],"endpoints":[],"model_routes":[],"mcp_servers":[],"mcp_credentials":[],"settings":[{"key":"redaction_enabled","version":1,"value":true,"updated_at":null}],"user_redaction_configs":[]}"#,
            "the normalized domain bytes drop the raw store domain and keep field order"
        );
        normalized
            .validate()
            .expect("normalized snapshot validates");
        let frozen: LegacyV2Domains =
            serde_json::from_str(LEGACY_DOMAINS_JSON).expect("frozen legacy domains decode");
        assert_eq!(
            serde_json::to_string(&frozen).expect("frozen legacy domains encode"),
            LEGACY_DOMAINS_JSON,
            "the pinned pre-grant bytes are unchanged"
        );
    }

    #[test]
    fn the_pinned_money_payload_is_rejected_before_normalization_when_tampered() {
        let normalized = normalize(LEGACY_MONEY_PAYLOAD_JSON.as_bytes()).expect("money payload");
        let provenance = normalized.manifest.provenance.as_ref().expect("provenance");
        assert_eq!(
            provenance.source_payload_fingerprint,
            LEGACY_MONEY_FINGERPRINT
        );
        assert!(
            provenance
                .discarded
                .iter()
                .any(|note| note.name == "mcp_credentials.retired_fields" && note.records == 1),
            "the non-default money values are counted as discarded: {:?}",
            provenance.discarded
        );
        assert!(
            !serde_json::to_string(&normalized.domains)
                .expect("normalized domains")
                .contains("default_cost")
        );

        let tampered = LEGACY_MONEY_PAYLOAD_JSON.replace(LEGACY_MONEY_FINGERPRINT, &"0".repeat(64));
        assert!(
            matches!(
                normalize(tampered.as_bytes()),
                Err("legacy snapshot payload fingerprint mismatch")
            ),
            "the original fingerprint is verified before any reshaping"
        );
        let frozen: LegacyV2Domains =
            serde_json::from_str(LEGACY_MONEY_DOMAINS_JSON).expect("frozen money domains decode");
        assert_eq!(
            serde_json::to_string(&frozen).expect("frozen money domains encode"),
            LEGACY_MONEY_DOMAINS_JSON,
            "the pinned money bytes are unchanged"
        );
        assert_eq!(frozen.mcp_credentials[0].default_cost, 0.25);
        assert!(frozen.mcp_credentials[0].strict_mode);
    }

    #[test]
    fn an_unknown_domain_set_or_version_is_rejected() {
        let unknown = LEGACY_PAYLOAD_JSON.replace("\"raw_object_store\"", "\"raw_payloads\"");
        assert!(matches!(
            normalize(unknown.as_bytes()),
            Err("unexpected snapshot domains")
        ));
        let wrong_version = LEGACY_PAYLOAD_JSON.replace(
            "\"format_version\":2",
            &format!("\"format_version\":{SNAPSHOT_FORMAT_VERSION}"),
        );
        assert!(matches!(
            normalize(wrong_version.as_bytes()),
            Err("unexpected payload version for the legacy reader")
        ));
        let duplicate = LEGACY_PAYLOAD_JSON.replace(
            "{\"name\":\"raw_object_store\",\"records\":0,\"unrecoverable_secrets\":0}",
            "{\"name\":\"raw_object_store\",\"records\":0,\"unrecoverable_secrets\":0},{\"name\":\"raw_object_store\",\"records\":0,\"unrecoverable_secrets\":0}",
        );
        assert!(matches!(
            normalize(duplicate.as_bytes()),
            Err("duplicate snapshot domain")
        ));
    }

    fn snapshot_manifest(domains: &LegacyV2Domains) -> SnapshotManifest {
        SnapshotManifest {
            format_version: LEGACY_FORMAT_VERSION,
            backend_kind: crate::db::config_repository::snapshot::ConfigBackendKind::Postgres,
            exported_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("timestamp"),
            payload_fingerprint: fingerprint_of(domains).expect("fingerprint"),
            domains: POST_GRANT_DOMAIN_NAMES
                .iter()
                .map(|name| super::super::manifest::SnapshotDomainSummary {
                    name: (*name).to_string(),
                    records: 0,
                    unrecoverable_secrets: 0,
                })
                .collect(),
            provenance: None,
        }
    }

    #[test]
    fn a_verified_post_grant_payload_keeps_its_personal_grant() {
        let mut domains: LegacyV2Domains =
            serde_json::from_str(LEGACY_MONEY_DOMAINS_JSON).expect("money domains");
        domains.mcp_oauth =
            vec![crate::db::config_repository::archive_test_fixtures::sample_grant()];
        let manifest = snapshot_manifest(&domains);
        let payload = serde_json::json!({
            "manifest": manifest,
            // Flatten the domain fields the same way the codec payload does.
            "users": domains.users,
            "client_keys": domains.client_keys,
            "endpoints": domains.endpoints,
            "model_routes": domains.model_routes,
            "mcp_servers": domains.mcp_servers,
            "mcp_credentials": domains.mcp_credentials,
            "settings": domains.settings,
            "user_redaction_configs": domains.user_redaction_configs,
            "raw_object_store": domains.raw_object_store,
            "mcp_oauth": domains.mcp_oauth,
        });
        let normalized = normalize(&serde_json::to_vec(&payload).expect("payload json"))
            .expect("post-grant payload normalizes");
        assert_eq!(normalized.domains.mcp_oauth.len(), 1);
        let grant = &normalized.domains.mcp_oauth[0];
        assert_eq!(grant.owner_user_id, 1);
        assert_eq!(
            grant.config_fingerprint,
            "fingerprint-of-the-archived-server"
        );
        assert_eq!(grant.credentials.scopes, ["mcp:read", "mcp:write"]);
        assert!(
            normalized
                .manifest
                .domains
                .iter()
                .any(|domain| domain.name == "mcp_oauth" && domain.records == 1),
            "the normalized manifest counts the grant"
        );
    }

    #[test]
    fn deployment_only_settings_are_discarded_and_counted() {
        let mut domains: LegacyV2Domains =
            serde_json::from_str(LEGACY_MONEY_DOMAINS_JSON).expect("money domains");
        let setting = |key: &str| SettingSnapshot {
            key: key.to_string(),
            version: 1,
            value: serde_json::Value::Null,
            updated_at: None,
        };
        domains.settings = vec![
            setting("redaction_enabled"),
            setting(LEGACY_RAW_OBJECT_STORE_SETTINGS_KEY),
            setting(SHARED_VALKEY_SETTINGS_KEY),
            setting(INTERNAL_COORDINATION_SETTING_KEYS[0]),
        ];
        domains.raw_object_store = Some(serde_json::json!({ "backend": "s3" }));
        let manifest = snapshot_manifest(&domains);
        let legacy = LegacyV2Snapshot { manifest, domains };
        let normalized = normalize_verified(legacy).expect("normalized");
        let provenance = normalized.manifest.provenance.as_ref().expect("provenance");
        let counts: std::collections::BTreeMap<&str, usize> = provenance
            .discarded
            .iter()
            .map(|note| (note.name.as_str(), note.records))
            .collect();
        assert_eq!(counts.get("raw_object_store"), Some(&1));
        assert_eq!(counts.get("settings.raw_object_store"), Some(&1));
        assert_eq!(counts.get("settings.coordination"), Some(&2));
        assert_eq!(normalized.domains.settings.len(), 1);
        assert_eq!(normalized.domains.settings[0].key, "redaction_enabled");
        let serialized = serde_json::to_string(&normalized.domains).expect("domains json");
        assert!(!serialized.contains("raw_object_store"));
        assert!(!serialized.contains(SHARED_VALKEY_SETTINGS_KEY));
    }
}
