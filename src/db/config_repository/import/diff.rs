//! Dry-run domain diff and import warnings.
//!
//! Counts and identifiers only: the preview must never echo a payload, so it
//! compares the archive's records with the running configuration by identity
//! and reports create/update/delete counts per domain.

use std::collections::BTreeSet;

use super::super::snapshot::{ConfigSnapshot, SecretRecovery, SnapshotDomains};
use super::{ConfigImportDomainDiff, DOMAIN_NAMES};

pub(super) fn diff_domains(
    archive: &SnapshotDomains,
    target: &SnapshotDomains,
) -> Vec<ConfigImportDomainDiff> {
    let mut diffs = Vec::with_capacity(DOMAIN_NAMES.len());
    for name in DOMAIN_NAMES {
        let (archive_ids, unrecoverable) = domain_ids(archive, name);
        let (target_ids, _) = domain_ids(target, name);
        let creates = archive_ids.difference(&target_ids).count();
        let updates = archive_ids.intersection(&target_ids).count();
        let deletes = target_ids.difference(&archive_ids).count();
        diffs.push(ConfigImportDomainDiff {
            name: name.to_string(),
            archive_records: archive_ids.len(),
            target_records: target_ids.len(),
            creates,
            updates,
            deletes,
            unrecoverable_secrets: unrecoverable,
        });
    }
    diffs
}

/// Identifier set for a domain. `raw_object_store` uses a single synthetic id
/// so presence/absence reads as 0 or 1 without exposing the config.
fn domain_ids(domains: &SnapshotDomains, name: &str) -> (BTreeSet<String>, usize) {
    let mut ids = BTreeSet::new();
    let mut unrecoverable = 0;
    match name {
        "users" => {
            for user in &domains.users {
                ids.insert(user.user_id.to_string());
                if user.password_hash.is_none() {
                    unrecoverable += 1;
                }
            }
        }
        "client_keys" => {
            for key in &domains.client_keys {
                ids.insert(key.key_id.to_string());
                if key.secret_state != SecretRecovery::Recovered {
                    unrecoverable += 1;
                }
            }
        }
        "endpoints" => {
            for endpoint in &domains.endpoints {
                ids.insert(endpoint.endpoint_id.to_string());
                if endpoint.api_key.is_none()
                    && endpoint.api_keys.iter().all(|key| key.api_key.is_none())
                {
                    unrecoverable += 1;
                }
            }
        }
        "model_routes" => {
            for route in &domains.model_routes {
                ids.insert(route.rule_id.to_string());
                unrecoverable += usize::from(route.targets.is_empty());
            }
        }
        "mcp_servers" => {
            for server in &domains.mcp_servers {
                ids.insert(server.server_id.to_string());
            }
        }
        "mcp_credentials" => {
            for credential in &domains.mcp_credentials {
                ids.insert(credential.credential_id.to_string());
            }
        }
        "relays" => {
            for relay in &domains.relays {
                ids.insert(relay.relay_id.to_string());
                let missing_key = relay.tls_mode != "off" && relay.client_key_pem.is_none();
                let missing_bridge = relay.bridge_encryption_mode == "required"
                    && relay.bridge_encryption_key.is_none();
                if missing_key || missing_bridge {
                    unrecoverable += 1;
                }
            }
        }
        "settings" => {
            for setting in &domains.settings {
                ids.insert(setting.key.clone());
            }
        }
        "user_redaction_configs" => {
            for config in &domains.user_redaction_configs {
                ids.insert(config.user_id.to_string());
            }
        }
        "raw_object_store" if domains.raw_object_store.is_some() => {
            ids.insert("raw_object_store".to_string());
        }
        _ => {}
    }
    (ids, unrecoverable)
}

pub(super) fn import_warnings(snapshot: &ConfigSnapshot) -> Vec<String> {
    let mut warnings = Vec::new();
    for domain in &snapshot.manifest.domains {
        if domain.unrecoverable_secrets > 0 {
            warnings.push(format!(
                "{}: {} record(s) carry an unrecoverable secret",
                domain.name, domain.unrecoverable_secrets
            ));
        }
    }
    if snapshot.domains.users.is_empty() {
        warnings.push(
            "the archive contains no users; administrator access will be lost until an account is created".to_string(),
        );
    }
    warnings
}
