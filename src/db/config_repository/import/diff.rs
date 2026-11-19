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

/// Identifier set for a domain. Only counts and identifiers are collected, so
/// the diff never exposes a payload.
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
        "mcp_oauth" => {
            for grant in &domains.mcp_oauth {
                ids.insert(format!("{}:{}", grant.server_id, grant.owner_user_id));
            }
        }
        "user_endpoint_settings" => {
            for setting in &domains.user_endpoint_settings {
                ids.insert(setting.user_id.to_string());
            }
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
    let declared: BTreeSet<&str> = snapshot
        .manifest
        .domains
        .iter()
        .map(|domain| domain.name.as_str())
        .collect();
    let provenance_missing: BTreeSet<&str> = snapshot
        .manifest
        .provenance
        .as_ref()
        .map(|provenance| {
            provenance
                .missing_domains
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let is_missing = |name: &str| !declared.contains(name) || provenance_missing.contains(name);
    if is_missing("user_endpoint_settings") {
        warnings.push(
            "the archive was written before explicit per-user endpoint preferences became \
             portable; those users keep the routing default until a preference is selected"
                .to_string(),
        );
    }
    if let Some(provenance) = &snapshot.manifest.provenance {
        warnings.push(format!(
            "the archive was written in payload format {} and normalized to format {}; only the \
             portable configuration is restored",
            provenance.source_format_version,
            crate::db::config_repository::snapshot::SNAPSHOT_FORMAT_VERSION
        ));
        for note in &provenance.discarded {
            warnings.push(format!(
                "{}: {} record(s) of deployment-only data were discarded during normalization",
                note.name, note.records
            ));
        }
        if is_missing("mcp_oauth") {
            warnings.push(
                "the format-2 archive carries no personal MCP OAuth grant; the affected servers \
                 may need reauthorization"
                    .to_string(),
            );
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::{diff_domains, import_warnings};
    use crate::db::config_repository::snapshot::{
        ConfigBackendKind, ConfigSnapshot, SnapshotDomainSummary, SnapshotDomains,
    };

    fn snapshot(domains: SnapshotDomains, names: &[&str]) -> ConfigSnapshot {
        let mut snapshot = ConfigSnapshot::new(ConfigBackendKind::Postgres, domains);
        snapshot.manifest.domains = names
            .iter()
            .map(|name| SnapshotDomainSummary {
                name: (*name).to_string(),
                records: 0,
                unrecoverable_secrets: 0,
            })
            .collect();
        snapshot
    }

    #[test]
    fn the_grant_domain_is_compared_by_server_and_owner() {
        let mut archive_domains = SnapshotDomains {
            ..crate::db::config_repository::snapshot_source::empty_domains()
        };
        archive_domains.mcp_oauth =
            vec![super::super::super::archive_test_fixtures::sample_grant()];
        let archive = snapshot(archive_domains, &super::super::DOMAIN_NAMES);
        let target = snapshot(
            crate::db::config_repository::snapshot_source::empty_domains(),
            &super::super::DOMAIN_NAMES,
        );
        let diffs = diff_domains(&archive.domains, &target.domains);
        let grant = diffs
            .iter()
            .find(|diff| diff.name == "mcp_oauth")
            .expect("the grant domain is compared");
        assert_eq!(grant.archive_records, 1);
        assert_eq!(grant.creates, 1);
        assert_eq!(grant.updates, 0);
        assert_eq!(grant.deletes, 0);
    }

    #[test]
    fn a_legacy_archive_warns_about_the_missing_grants() {
        let legacy = crate::db::config_repository::snapshot::legacy::normalize(
            crate::db::config_repository::archive_test_fixtures::LEGACY_PAYLOAD_JSON.as_bytes(),
        )
        .expect("legacy payload normalizes");
        let warnings = import_warnings(&legacy);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("personal MCP OAuth grants")
                    || warning.contains("personal MCP OAuth grant;")),
            "a normalized legacy archive warns that no grant is restored: {warnings:?}"
        );

        let current = snapshot(
            crate::db::config_repository::snapshot_source::empty_domains(),
            &super::super::DOMAIN_NAMES,
        );
        let warnings = import_warnings(&current);
        assert!(
            !warnings
                .iter()
                .any(|warning| warning.contains("personal MCP OAuth")),
            "a current archive needs no such warning"
        );
    }

    #[test]
    fn retired_credential_money_values_produce_a_provenance_warning_without_secrets() {
        let archived = crate::db::config_repository::snapshot::legacy::normalize(
            crate::db::config_repository::archive_test_fixtures::LEGACY_MONEY_PAYLOAD_JSON
                .as_bytes(),
        )
        .expect("money payload normalizes");
        let warnings = import_warnings(&archived);
        let warning = warnings
            .iter()
            .find(|warning| warning.contains("mcp_credentials.retired_fields"))
            .expect("the discarded money fields are reported");
        assert!(
            warning.contains("1 record(s)"),
            "the warning counts the affected records: {warning}"
        );
        assert!(
            !warning.contains("fc-archived-secret"),
            "the warning never echoes archived secret material: {warning}"
        );

        let current = snapshot(
            crate::db::config_repository::snapshot_source::empty_domains(),
            &super::super::DOMAIN_NAMES,
        );
        assert!(
            !import_warnings(&current)
                .iter()
                .any(|warning| warning.contains("retired_fields")),
            "a current archive needs no discarded-field warning"
        );
    }
}

#[cfg(test)]
mod preference_tests {
    use super::*;
    use crate::db::config_repository::snapshot::{
        ConfigBackendKind, ConfigSnapshot, UserEndpointSettingSnapshot,
    };

    fn old_nine() -> ConfigSnapshot {
        serde_json::from_slice(
            crate::db::config_repository::archive_test_fixtures::V3_NINE_PAYLOAD_JSON.as_bytes(),
        )
        .expect("frozen old V3 payload decodes")
    }

    #[test]
    fn preferences_are_compared_by_user_id() {
        let mut archive = crate::db::config_repository::snapshot_source::empty_domains();
        archive.user_endpoint_settings = vec![
            UserEndpointSettingSnapshot {
                user_id: 1,
                endpoint_id: None,
            },
            UserEndpointSettingSnapshot {
                user_id: 2,
                endpoint_id: Some(uuid::Uuid::new_v4()),
            },
        ];
        let mut target = crate::db::config_repository::snapshot_source::empty_domains();
        target.user_endpoint_settings = vec![
            UserEndpointSettingSnapshot {
                user_id: 2,
                endpoint_id: None,
            },
            UserEndpointSettingSnapshot {
                user_id: 3,
                endpoint_id: None,
            },
        ];
        let diff = diff_domains(&archive, &target)
            .into_iter()
            .find(|diff| diff.name == "user_endpoint_settings")
            .expect("preference domain compared");
        assert_eq!(diff.archive_records, 2);
        assert_eq!(diff.target_records, 2);
        assert_eq!(diff.creates, 1);
        assert_eq!(diff.updates, 1);
        assert_eq!(diff.deletes, 1);
    }

    #[test]
    fn missing_domains_come_from_manifest_metadata_not_empty_records() {
        let old = import_warnings(&old_nine());
        assert!(
            old.iter()
                .any(|warning| warning.contains("per-user endpoint preferences")),
            "an old native archive warns about the missing preference domain: {old:?}"
        );
        assert!(
            !old.iter()
                .any(|warning| warning.contains("personal MCP OAuth")),
            "the old native archive declares the grant domain, so no grant warning: {old:?}"
        );

        let legacy = crate::db::config_repository::snapshot::legacy::normalize(
            crate::db::config_repository::archive_test_fixtures::LEGACY_PAYLOAD_JSON.as_bytes(),
        )
        .expect("pre-grant payload normalizes");
        let legacy_warnings = import_warnings(&legacy);
        assert!(
            legacy_warnings
                .iter()
                .any(|warning| warning.contains("per-user endpoint preferences")),
            "a pre-grant archive also misses preferences: {legacy_warnings:?}"
        );
        assert!(
            legacy_warnings
                .iter()
                .any(|warning| warning.contains("personal MCP OAuth grant")),
            "a pre-grant archive uses precise missing-domain metadata: {legacy_warnings:?}"
        );

        let current = old_nine();
        let mut current = ConfigSnapshot::new(ConfigBackendKind::Postgres, current.domains);
        current.manifest.domains.push(
            crate::db::config_repository::snapshot::SnapshotDomainSummary {
                name: "user_endpoint_settings".to_string(),
                records: 0,
                unrecoverable_secrets: 0,
            },
        );
        let current_warnings = import_warnings(&current);
        assert!(
            !current_warnings
                .iter()
                .any(|warning| warning.contains("per-user endpoint preferences")),
            "a current archive with empty preferences needs no warning: {current_warnings:?}"
        );
    }
}
