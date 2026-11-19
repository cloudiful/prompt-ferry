//! Database-wide configuration revision read and notification policy.
//!
//! The database is the authority: a configuration write bumps one singleton
//! row and notifies the channel inside the same transaction. A listening node
//! treats the payload as a hint only, reads the authoritative revision through
//! this module and refreshes its snapshots when that revision is newer.

use anyhow::{Result, anyhow, bail};
use sea_orm::EntityTrait;
use sqlx::PgPool;
use sqlx::postgres::PgListener;

use super::connection::POSTGRES_CONNECT_TIMEOUT;
use super::entities::postgres::config_revision as entity;
use super::seaorm::{native_error, postgres_connection};

/// The notification channel of the configuration revision.
pub const CONFIG_REVISION_CHANNEL: &str = "prompt_ferry_config_revision";

/// The largest revision text accepted as a notification hint.
const MAX_REVISION_HINT_DIGITS: usize = 19;

/// The revision this process has fully applied.
///
/// It only advances after a complete successful refresh; a failed reload keeps
/// the previous value, so the next poll or notification retries it.
///
/// A failed refresh is remembered as well: the revision itself did not change,
/// so the retry policy cannot be derived from the revision alone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppliedRevision {
    revision: Option<i64>,
    failed: bool,
}

impl AppliedRevision {
    pub fn get(self) -> Option<i64> {
        self.revision
    }

    /// Whether `revision` is newer than the revision that was applied.
    ///
    /// A failed refresh always needs another attempt, even when the revision
    /// did not change since the last successful one.
    pub fn needs_refresh(self, revision: i64) -> bool {
        self.failed || self.revision.is_none_or(|applied| revision > applied)
    }

    /// Record a successfully applied revision.
    ///
    /// The applied value never moves backwards.
    pub fn commit(&mut self, revision: i64) {
        self.revision = Some(
            self.revision
                .map_or(revision, |applied| applied.max(revision)),
        );
        self.failed = false;
    }

    /// Record that the last refresh failed.
    ///
    /// The applied revision stays where it was, and the next poll retries the
    /// same revision instead of waiting for a newer one.
    pub fn mark_failed(&mut self) {
        self.failed = true;
    }

    /// Whether the last refresh failed.
    pub fn has_failed(self) -> bool {
        self.failed
    }
}

/// Parse a notification payload as a bounded, non-negative numeric revision.
///
/// Any other payload is not a revision hint and is never logged verbatim.
pub fn parse_revision_hint(payload: &str) -> Option<i64> {
    let payload = payload.trim();
    if payload.is_empty()
        || payload.len() > MAX_REVISION_HINT_DIGITS
        || !payload.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    payload
        .parse::<i64>()
        .ok()
        .filter(|revision| *revision >= 0)
}

/// Whether a changed shared Valkey setting requires a coordinated restart.
///
/// The active URL, TTL and coordination revision are never replaced in place.
pub fn shared_valkey_restart_required(desired: Option<i64>, applied: Option<i64>) -> bool {
    desired != applied
}

/// Read the authoritative configuration revision from the gated application pool.
pub async fn read_revision(pool: &PgPool) -> Result<i64> {
    let db = postgres_connection(pool);
    let row = entity::Entity::find_by_id(1_i16)
        .one(&db)
        .await
        .map_err(native_error)?
        .ok_or_else(|| anyhow!("the configuration revision row is missing"))?;
    if row.revision < 0 {
        bail!("the configuration revision is negative");
    }
    Ok(row.revision)
}

/// One dedicated control connection that only LISTENs on the revision channel.
///
/// It is never a session of the gated application pool, so it holds no shared
/// application barrier and cannot delay an application session or an upgrade.
pub struct ConfigRevisionListener {
    listener: PgListener,
}

impl ConfigRevisionListener {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let mut listener =
            tokio::time::timeout(POSTGRES_CONNECT_TIMEOUT, PgListener::connect(database_url))
                .await
                .map_err(|_| anyhow!("the configuration revision listener connection timed out"))?
                .map_err(|_| anyhow!("failed to connect the configuration revision listener"))?;
        listener
            .listen(CONFIG_REVISION_CHANNEL)
            .await
            .map_err(|_| anyhow!("failed to subscribe to the configuration revision channel"))?;
        Ok(Self { listener })
    }

    /// Wait for one notification and return its parsed revision hint.
    pub async fn recv(&mut self) -> Result<Option<i64>> {
        let notification = self
            .listener
            .recv()
            .await
            .map_err(|_| anyhow!("the configuration revision listener failed"))?;
        Ok(parse_revision_hint(notification.payload()))
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::sea_query::PostgresQueryBuilder;
    use sea_orm::{IdenStatic, Iterable};

    use super::{AppliedRevision, parse_revision_hint, shared_valkey_restart_required};
    use crate::db::entities::postgres::{
        client_keys, config_revision, endpoint_api_keys, endpoint_oauth_tokens, mcp_credentials,
        mcp_servers, model_endpoint_rules, model_route_targets, provider_endpoints,
        user_endpoint_settings, user_redaction_configs, users, worker_settings,
    };

    struct TriggerTable {
        table: &'static str,
        config: &'static [&'static str],
        ignored: &'static [&'static str],
    }

    const TRIGGER_TABLES: &[TriggerTable] = &[
        TriggerTable {
            table: "users",
            config: &[
                "login_name",
                "password_hash",
                "display_name",
                "is_admin",
                "is_active",
            ],
            ignored: &["user_id", "created_at", "updated_at"],
        },
        TriggerTable {
            table: "client_keys",
            config: &[
                "user_id",
                "key_prefix",
                "key_hash",
                "label",
                "enabled",
                "secret",
            ],
            ignored: &["key_id", "last_used_at", "created_at"],
        },
        TriggerTable {
            table: "provider_endpoints",
            config: &[
                "scope",
                "owner_user_id",
                "name",
                "base_url",
                "native_api",
                "native_api_source",
                "api_key",
                "enabled",
                "key_lb_enabled",
                "provider",
                "provider_region",
                "mcp_enabled",
                "service_tier",
                "proxy_url",
                "active_windows",
                "admin_api_key",
            ],
            ignored: &["endpoint_id", "created_at", "updated_at"],
        },
        TriggerTable {
            table: "endpoint_api_keys",
            config: &["endpoint_id", "key_label", "api_key", "position", "enabled"],
            ignored: &["key_id", "created_at", "updated_at"],
        },
        TriggerTable {
            table: "endpoint_oauth_tokens",
            config: &["access_token", "refresh_token", "expires_at"],
            ignored: &["endpoint_id", "created_at", "updated_at"],
        },
        TriggerTable {
            table: "model_endpoint_rules",
            config: &[
                "scope",
                "owner_user_id",
                "model_pattern",
                "endpoint_id",
                "priority",
                "enabled",
                "routing_strategy",
            ],
            ignored: &["rule_id", "created_at", "updated_at"],
        },
        TriggerTable {
            table: "model_route_targets",
            config: &[
                "rule_id",
                "endpoint_id",
                "position",
                "enabled",
                "upstream_model",
                "proxy_url_override",
                "active_windows",
                "dev_system_normalize",
                "native_api",
                "thinking_effort_override",
                "compact_mode",
                "thinking_downgrade_enabled",
                "service_tier",
                "embedding_dimensions_override",
                "prefer_new_sessions",
            ],
            ignored: &["target_id", "created_at", "updated_at"],
        },
        TriggerTable {
            table: "mcp_servers",
            config: &[
                "name",
                "transport",
                "url",
                "command",
                "args",
                "env_json",
                "enabled",
                "timeout_ms",
                "disabled_tools",
                "disabled_resources",
                "scope",
                "owner_user_id",
                "tool_filter_mode",
                "allowed_tools",
                "http_headers_json",
                "aggregate_naming_mode",
                "bearer_tokens_json",
                "lifecycle_policy",
                "lifecycle_manual_protocol_version",
                "source_endpoint_id",
                "auth_mode",
                "basic_username",
                "basic_password",
                "provider_kind",
                "proxy_url",
            ],
            ignored: &[
                "server_id",
                "created_at",
                "updated_at",
                "lifecycle_learned_mode",
                "lifecycle_learned_protocol_version",
                "lifecycle_learned_for_updated_at",
                "lifecycle_learned_at",
            ],
        },
        TriggerTable {
            table: "mcp_credentials",
            config: &[
                "server_id",
                "credential_label",
                "secret",
                "position",
                "enabled",
                "provider_kind",
            ],
            ignored: &[
                "credential_id",
                "provider_remaining",
                "provider_synced_at",
                "provider_reset_at",
                "cooldown_until",
                "last_error",
                "last_error_at",
                "created_at",
                "updated_at",
            ],
        },
        TriggerTable {
            table: "user_redaction_configs",
            config: &["user_id", "config"],
            ignored: &["updated_at"],
        },
        TriggerTable {
            table: "user_endpoint_settings",
            config: &["user_id", "endpoint_id"],
            ignored: &["updated_at"],
        },
        TriggerTable {
            table: "worker_settings",
            config: &["setting_key", "setting_value"],
            ignored: &["updated_at"],
        },
    ];

    const RUNTIME_COLUMNS: &[(&str, &[&str])] = &[
        ("client_keys", &["last_used_at"]),
        (
            "mcp_servers",
            &[
                "lifecycle_learned_mode",
                "lifecycle_learned_protocol_version",
                "lifecycle_learned_for_updated_at",
                "lifecycle_learned_at",
            ],
        ),
        (
            "mcp_credentials",
            &[
                "provider_remaining",
                "provider_synced_at",
                "provider_reset_at",
                "cooldown_until",
                "last_error",
                "last_error_at",
            ],
        ),
    ];

    fn entity_columns(table: &str) -> Vec<&'static str> {
        match table {
            "users" => users::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "client_keys" => client_keys::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "provider_endpoints" => provider_endpoints::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "endpoint_api_keys" => endpoint_api_keys::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "endpoint_oauth_tokens" => endpoint_oauth_tokens::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "model_endpoint_rules" => model_endpoint_rules::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "model_route_targets" => model_route_targets::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "mcp_servers" => mcp_servers::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "mcp_credentials" => mcp_credentials::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "user_redaction_configs" => user_redaction_configs::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "user_endpoint_settings" => user_endpoint_settings::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            "worker_settings" => worker_settings::Column::iter()
                .map(|column| column.as_str())
                .collect(),
            other => panic!("unexpected configuration table {other}"),
        }
    }

    #[test]
    fn revision_hints_accept_only_bounded_non_negative_numbers() {
        assert_eq!(parse_revision_hint("0"), Some(0));
        assert_eq!(parse_revision_hint(" 42 "), Some(42));
        assert_eq!(parse_revision_hint("9223372036854775807"), Some(i64::MAX));
        for payload in [
            "",
            "   ",
            "-1",
            "+1",
            "1.0",
            "1 2",
            "42x",
            "9223372036854775808",
            "00000000000000000001",
        ] {
            assert_eq!(parse_revision_hint(payload), None, "{payload:?}");
        }
    }

    #[test]
    fn applied_revision_only_advances_after_a_successful_refresh() {
        let mut applied = AppliedRevision::default();
        assert!(applied.needs_refresh(1));
        assert_eq!(
            applied.get(),
            None,
            "a read alone must not mark the revision"
        );

        applied.commit(5);
        assert!(!applied.needs_refresh(5));
        assert!(
            !applied.needs_refresh(4),
            "an older revision is never applied"
        );
        assert!(applied.needs_refresh(6));

        applied.commit(4);
        assert_eq!(
            applied.get(),
            Some(5),
            "the applied revision never moves backwards"
        );
    }

    #[test]
    fn a_failed_refresh_is_retried_on_the_same_revision() {
        let mut applied = AppliedRevision::default();
        applied.commit(5);
        assert!(!applied.needs_refresh(5));

        applied.mark_failed();
        assert!(
            applied.needs_refresh(5),
            "a failed refresh of the applied revision is retried"
        );
        assert_eq!(
            applied.get(),
            Some(5),
            "a failed refresh does not advance the applied revision"
        );

        applied.commit(5);
        assert!(
            !applied.needs_refresh(5),
            "a successful refresh clears the failed state"
        );
        assert!(!applied.has_failed());
    }

    #[test]
    fn a_shared_valkey_change_requires_a_restart_and_is_never_hot_switched() {
        assert!(shared_valkey_restart_required(Some(2), Some(1)));
        assert!(shared_valkey_restart_required(Some(1), None));
        assert!(!shared_valkey_restart_required(Some(1), Some(1)));
        assert!(!shared_valkey_restart_required(None, None));
    }

    #[test]
    fn the_per_transaction_guard_bumps_once_per_transaction() {
        fn transaction_bumps(previous: Option<&str>, current: &str) -> bool {
            previous != Some(current)
        }

        assert!(transaction_bumps(None, "10"));
        assert!(!transaction_bumps(Some("10"), "10"));
        assert!(transaction_bumps(Some("10"), "11"));
    }

    #[test]
    fn every_configuration_column_is_triggered_or_explicitly_ignored() {
        for table in TRIGGER_TABLES {
            let columns = entity_columns(table.table);
            for column in &columns {
                assert!(
                    table.config.contains(column) || table.ignored.contains(column),
                    "{} column {column} is neither triggered nor ignored",
                    table.table
                );
            }
            for column in table.config {
                assert!(
                    columns.contains(column),
                    "{} trigger column {column} does not exist on the entity",
                    table.table
                );
            }
            for column in table.ignored {
                assert!(
                    !table.config.contains(column),
                    "{} column {column} is both triggered and ignored",
                    table.table
                );
            }
        }
    }

    #[test]
    fn runtime_state_columns_are_never_configuration_trigger_columns() {
        for (table, columns) in RUNTIME_COLUMNS {
            let trigger = TRIGGER_TABLES
                .iter()
                .find(|trigger| trigger.table == *table)
                .unwrap_or_else(|| panic!("{table} has no trigger declaration"));
            let entity = entity_columns(table);
            for column in *columns {
                assert!(
                    entity.contains(column),
                    "{table} has no runtime column {column}"
                );
                assert!(
                    !trigger.config.contains(column),
                    "{table} must not trigger on runtime column {column}"
                );
                assert!(
                    trigger.ignored.contains(column),
                    "{table} must explicitly ignore runtime column {column}"
                );
            }
        }
    }

    #[test]
    fn the_migration_uses_the_declared_trigger_columns_and_tables() {
        let sql = include_str!("../../migrations/20261118031011_node_config_revision.up.sql");
        assert_eq!(
            sql.matches("create trigger ").count(),
            TRIGGER_TABLES.len() * 2,
            "every configuration table needs one write and one update trigger"
        );
        for table in TRIGGER_TABLES
            .iter()
            .filter(|table| table.table != "mcp_credentials")
        {
            let update = format!(
                "after update of {} on public.{}",
                table.config.join(", "),
                table.table
            );
            let start = sql
                .find(&update)
                .unwrap_or_else(|| panic!("missing update trigger for {}", table.table));
            let block = &sql[start + update.len()..];
            let when = &block[..block
                .find("execute function")
                .unwrap_or_else(|| panic!("unterminated update trigger for {}", table.table))];
            for column in table.config {
                let comparison = format!("old.{column} is distinct from new.{column}");
                assert!(
                    when.contains(&comparison),
                    "{} update trigger omits {column}",
                    table.table
                );
            }
            for column in table.ignored {
                let comparison = format!("old.{column} is distinct from new.{column}");
                assert!(
                    !when.contains(&comparison),
                    "{} update trigger compares ignored column {column}",
                    table.table
                );
            }
            let write = format!("after insert or delete on public.{}", table.table);
            assert!(
                sql.contains(&write),
                "missing write trigger for {}",
                table.table
            );
        }

        // The decision trigger for the retired MCP credential fields is replaced
        // by the fresh Timescale migration; the historical file stays untouched.
        let credentials = TRIGGER_TABLES
            .iter()
            .find(|table| table.table == "mcp_credentials")
            .expect("mcp_credentials declaration");
        assert!(sql.contains(
            "after update of server_id, credential_label, secret, position, enabled, provider_kind, default_cost, strict_mode on public.mcp_credentials"
        ));
        let replacement =
            include_str!("../../migrations/20261118133636_request_family_timescale.up.sql")
                .to_lowercase();
        assert!(replacement.contains(
            "drop trigger if exists trg_cfg_rev_mcp_credentials_update on public.mcp_credentials"
        ));
        let update = format!(
            "after update of {} on public.mcp_credentials",
            credentials.config.join(", ")
        );
        let start = replacement
            .find(&update)
            .unwrap_or_else(|| panic!("missing replacement update trigger for mcp_credentials"));
        let block = &replacement[start + update.len()..];
        let when = &block[..block
            .find("execute function")
            .unwrap_or_else(|| panic!("unterminated replacement update trigger"))];
        for column in credentials.config {
            let comparison = format!("old.{column} is distinct from new.{column}");
            assert!(
                when.contains(&comparison),
                "mcp_credentials replacement update trigger omits {column}"
            );
        }
        for retired in [
            "default_cost",
            "strict_mode",
            "billing_period_start",
            "billing_period_end",
        ] {
            assert!(
                !when.contains(retired),
                "replacement trigger compares {retired}"
            );
        }

        for excluded in [
            "request_records",
            "usage_charges",
            "quota_key_observations",
            "quota_snapshots",
            "mcp_runtime_sessions",
            "replay_snapshots",
            "prompt_logs",
        ] {
            assert!(
                !sql.contains(&format!("on public.{excluded}\n")),
                "the configuration revision must not trigger on {excluded}"
            );
        }
    }

    #[test]
    fn the_migration_bumps_and_notifies_inside_the_modifying_transaction() {
        let sql = include_str!("../../migrations/20261118031011_node_config_revision.up.sql");
        assert!(sql.contains("create table public.prompt_ferry_config_revision"));
        assert!(sql.contains("pg_current_xact_id()::text"));
        assert!(sql.contains("for update"));
        assert!(
            sql.contains("perform pg_notify('prompt_ferry_config_revision', next_revision::text)")
        );
    }

    #[test]
    fn the_entity_declaration_matches_the_singleton_schema() {
        let sql = config_revision::create_table_statement().to_string(PostgresQueryBuilder);
        for expected in [
            "\"id\" smallint NOT NULL PRIMARY KEY",
            "\"revision\" bigint NOT NULL DEFAULT (0)",
            "\"last_notify_txid\" text",
            "\"updated_at\" timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "CHECK (id = 1)",
            "CHECK (revision >= 0)",
        ] {
            assert!(sql.contains(expected), "missing {expected}: {sql}");
        }
    }
}
