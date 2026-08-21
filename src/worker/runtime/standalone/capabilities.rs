#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StandaloneFeature {
    DbBackedUsage,
    RawPayloadRetention,
    Approvals,
    DatabaseQuotas,
    Mcp,
    Billing,
    DurableReplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StandaloneFeatureDiagnostic {
    pub(crate) feature: StandaloneFeature,
    pub(crate) name: &'static str,
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}

impl StandaloneFeature {
    #[allow(dead_code)]
    pub(crate) const ALL: [Self; 7] = [
        Self::DbBackedUsage,
        Self::RawPayloadRetention,
        Self::Approvals,
        Self::DatabaseQuotas,
        Self::Mcp,
        Self::Billing,
        Self::DurableReplay,
    ];
}

pub(crate) fn diagnostic(feature: StandaloneFeature) -> StandaloneFeatureDiagnostic {
    match feature {
        StandaloneFeature::DbBackedUsage => StandaloneFeatureDiagnostic {
            feature,
            name: "db_backed_usage",
            code: "sqlite_request_records_unavailable",
            message: "durable request and usage records are unavailable with SQLite; recent summaries are kept in bounded memory only",
        },
        StandaloneFeature::RawPayloadRetention => StandaloneFeatureDiagnostic {
            feature,
            name: "raw_payload_retention",
            code: "sqlite_raw_payload_retention_unavailable",
            message: "raw request and response payload retention is unavailable with SQLite",
        },
        StandaloneFeature::Approvals => StandaloneFeatureDiagnostic {
            feature,
            name: "approvals",
            code: "sqlite_approvals_unavailable",
            message: "approval workflows are unavailable with SQLite",
        },
        StandaloneFeature::DatabaseQuotas => StandaloneFeatureDiagnostic {
            feature,
            name: "database_quotas",
            code: "sqlite_database_quotas_unavailable",
            message: "database-backed quotas are unavailable with SQLite",
        },
        StandaloneFeature::Mcp => StandaloneFeatureDiagnostic {
            feature,
            name: "mcp",
            code: "sqlite_mcp_quota_unavailable",
            message: "MCP quota and usage ledgers are unavailable with SQLite",
        },
        StandaloneFeature::Billing => StandaloneFeatureDiagnostic {
            feature,
            name: "billing",
            code: "sqlite_billing_unavailable",
            message: "billing persistence is unavailable with SQLite",
        },
        StandaloneFeature::DurableReplay => StandaloneFeatureDiagnostic {
            feature,
            name: "durable_replay",
            code: "replay_unavailable",
            message: "stored conversation content has expired or is unavailable",
        },
    }
}

#[allow(dead_code)]
pub(crate) fn diagnostics() -> [StandaloneFeatureDiagnostic; 7] {
    StandaloneFeature::ALL.map(diagnostic)
}

#[cfg(test)]
mod tests {
    use super::{StandaloneFeature, diagnostics};

    #[test]
    fn every_unsupported_feature_has_a_stable_diagnostic() {
        let diagnostics = diagnostics();
        assert_eq!(diagnostics.len(), StandaloneFeature::ALL.len());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "replay_unavailable")
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.code.starts_with("standalone_"))
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.name.is_empty())
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.message.is_empty())
        );
    }
}
