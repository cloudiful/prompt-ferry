//! Native SeaORM entities for migrated configuration and authentication tables.
//!
//! The batch covers the PostgreSQL configuration, user and client-key tables
//! and the standalone SQLite configuration, user and client-key tables. Each
//! entity module carries its own native schema declaration: the entity model is
//! the column contract, and the colocated `sea_query` statements add the checks
//! and indexes that entity attributes cannot express. No entity declares a
//! `Relation`, so no foreign key is ever generated; the applied foreign keys stay
//! owned by the existing migration contract.

pub mod postgres;
pub mod sqlite;

#[cfg(test)]
mod tests {
    use sea_orm::sea_query::{PostgresQueryBuilder, SqliteQueryBuilder};

    use super::postgres::client_keys;
    use super::postgres::config_archive_audit;
    use super::postgres::endpoint_api_keys;
    use super::postgres::managed_relays;
    use super::postgres::mcp_servers;
    use super::postgres::model_endpoint_rules;
    use super::postgres::model_route_targets;
    use super::postgres::provider_endpoints;
    use super::postgres::users;
    use super::sqlite::standalone_client_keys;
    use super::sqlite::standalone_endpoint_keys;
    use super::sqlite::standalone_mcp_servers;
    use super::sqlite::standalone_model_route_targets;
    use super::sqlite::standalone_model_routes;
    use super::sqlite::standalone_provider_endpoints;
    use super::sqlite::standalone_users;

    #[test]
    fn credential_and_user_setting_postgres_declarations_preserve_nullability_and_defaults() {
        use super::postgres::{endpoint_oauth_tokens, user_endpoint_settings};
        let token = endpoint_oauth_tokens::create_table_statement().to_string(PostgresQueryBuilder);
        for expected in [
            "\"endpoint_id\" uuid NOT NULL",
            "\"access_token\" text,",
            "\"refresh_token\" text,",
            "\"expires_at\" timestamp with time zone,",
            "\"created_at\" timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "\"updated_at\" timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "PRIMARY KEY",
        ] {
            assert!(token.contains(expected), "missing {expected}: {token}");
        }
        assert!(!token.contains("FOREIGN KEY"));
        let setting =
            user_endpoint_settings::create_table_statement().to_string(PostgresQueryBuilder);
        for expected in [
            "\"user_id\" bigint NOT NULL",
            "\"endpoint_id\" uuid,",
            "\"updated_at\" timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "PRIMARY KEY",
        ] {
            assert!(setting.contains(expected), "missing {expected}: {setting}");
        }
        assert!(!setting.contains("FOREIGN KEY"));
    }

    #[test]
    fn sqlite_credential_declarations_preserve_nullable_primary_key_and_envelopes() {
        use super::sqlite::{standalone_endpoint_admin_keys, standalone_endpoint_oauth_tokens};
        for sql in [
            standalone_endpoint_admin_keys::create_table_statement().to_string(SqliteQueryBuilder),
            standalone_endpoint_oauth_tokens::create_table_statement()
                .to_string(SqliteQueryBuilder),
        ] {
            assert!(sql.contains("\"endpoint_id\" text PRIMARY KEY"), "{sql}");
            assert!(!sql.contains("FOREIGN KEY"), "{sql}");
            assert!(!sql.contains("blob NOT NULL"), "{sql}");
            assert!(!sql.contains("integer NOT NULL"), "{sql}");
            assert!(
                sql.contains("\"created_at\" text NOT NULL DEFAULT CURRENT_TIMESTAMP"),
                "{sql}"
            );
            assert!(
                sql.contains("\"updated_at\" text NOT NULL DEFAULT CURRENT_TIMESTAMP"),
                "{sql}"
            );
        }
    }

    #[test]
    fn postgres_provider_endpoint_declaration_matches_the_applied_table() {
        let table = provider_endpoints::create_table_statement().to_owned();
        let sql = table.to_string(PostgresQueryBuilder);
        for expected in [
            "\"endpoint_id\" uuid NOT NULL DEFAULT ((md5(random()::text || clock_timestamp()::text)::uuid))",
            "\"scope\" text NOT NULL",
            "\"owner_user_id\" bigint",
            "\"name\" text NOT NULL",
            "\"base_url\" text NOT NULL",
            "\"native_api\" text NOT NULL DEFAULT ('chat')",
            "\"native_api_source\" text NOT NULL DEFAULT ('manual')",
            "\"api_key\" text NOT NULL",
            "\"enabled\" bool NOT NULL DEFAULT (TRUE)",
            "\"created_at\" timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "\"updated_at\" timestamp with time zone NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "\"key_lb_enabled\" bool NOT NULL DEFAULT (FALSE)",
            "\"provider\" text NOT NULL DEFAULT ('generic')",
            "\"provider_region\" text",
            "\"mcp_enabled\" bool NOT NULL DEFAULT (FALSE)",
            "\"service_tier\" text",
            "\"proxy_url\" text",
            "\"active_windows\" text",
            "\"admin_api_key\" text",
        ] {
            assert!(sql.contains(expected), "missing {expected}: {sql}");
        }
        // The applied table declares no name uniqueness and no service tier
        // default or check.
        assert!(
            !sql.contains("UNIQUE"),
            "no unique constraint allowed: {sql}"
        );
        assert!(
            !sql.contains("service_tier\" text NOT NULL"),
            "service tier must stay nullable: {sql}"
        );
        for expected in [
            "provider_endpoints_scope_check",
            "ck_provider_endpoint_owner",
            "ck_provider_endpoints_native_api",
            "ck_provider_endpoints_native_api_source",
            "ck_provider_endpoints_provider",
            "ck_provider_endpoints_provider_region",
        ] {
            assert!(sql.contains(expected), "missing {expected}: {sql}");
        }
        assert!(
            !sql.contains("FOREIGN KEY"),
            "the applied foreign key stays owned by the migration: {sql}"
        );

        let rendered = provider_endpoints::create_index_statements()
            .iter()
            .map(|index| index.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered.iter().any(|sql| {
                sql.contains("idx_provider_endpoints_scope_user")
                    && sql.contains("(\"scope\", \"owner_user_id\", \"enabled\")")
            }),
            "applied scope, owner and enabled index missing: {rendered:?}"
        );
    }

    #[test]
    fn sqlite_provider_endpoint_declaration_matches_the_applied_table() {
        let table = standalone_provider_endpoints::create_table_statement().to_owned();
        let sql = table.to_string(SqliteQueryBuilder);
        for expected in [
            "\"provider\" IN ('generic', 'minimax', 'command_code', 'opencode_go', 'openrouter', 'glm', 'deepseek', 'openai')",
            "\"native_api\" IN ('auto', 'anthropic_messages', 'chat', 'responses', 'realtime', 'embeddings')",
            "\"native_api_source\" IN ('auto', 'detected', 'manual')",
            "\"provider_region\" IS NULL OR \"provider_region\" IN ('cn', 'global')",
            "\"name\" text NOT NULL UNIQUE",
        ] {
            assert!(sql.contains(expected), "missing {expected}: {sql}");
        }
        assert!(
            !sql.contains("FOREIGN KEY"),
            "the applied table carries no foreign key: {sql}"
        );
        let endpoint_id_start = sql.find("\"endpoint_id\"").expect("endpoint_id column");
        let endpoint_id_rest = &sql[endpoint_id_start..];
        let endpoint_id_end = endpoint_id_rest.find(',').unwrap_or(endpoint_id_rest.len());
        let endpoint_id_column = &endpoint_id_rest[..endpoint_id_end];
        assert!(
            !endpoint_id_column.to_ascii_lowercase().contains("not null"),
            "endpoint_id must keep the applied nullable TEXT PRIMARY KEY: {endpoint_id_column}"
        );
    }

    #[test]
    fn sqlite_endpoint_key_declaration_matches_the_applied_table() {
        let table = standalone_endpoint_keys::create_table_statement().to_owned();
        let sql = table.to_string(SqliteQueryBuilder);
        for expected in [
            "UNIQUE (\"endpoint_id\", \"key_label\")",
            "UNIQUE (\"endpoint_id\", \"position\")",
            "\"enabled\" integer NOT NULL CHECK (\"enabled\" IN (0, 1))",
            "\"api_key_ciphertext\" blob NOT NULL",
            "\"api_key_nonce\" blob NOT NULL",
            "\"api_key_key_version\" integer NOT NULL",
        ] {
            assert!(sql.contains(expected), "missing {expected}: {sql}");
        }
        assert!(
            !sql.contains("FOREIGN KEY"),
            "the applied foreign key stays owned by the migration: {sql}"
        );
        let key_id_start = sql.find("\"key_id\"").expect("key_id column");
        let key_id_rest = &sql[key_id_start..];
        let key_id_end = key_id_rest.find(',').unwrap_or(key_id_rest.len());
        let key_id_column = &key_id_rest[..key_id_end];
        assert!(
            !key_id_column.to_ascii_lowercase().contains("not null"),
            "key_id must keep the applied nullable TEXT PRIMARY KEY: {key_id_column}"
        );

        let rendered = standalone_endpoint_keys::create_index_statements()
            .iter()
            .map(|index| index.to_string(SqliteQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered.iter().any(|sql| {
                sql.contains("idx_standalone_endpoint_keys_endpoint")
                    && sql.contains("(\"endpoint_id\", \"enabled\", \"position\")")
            }),
            "applied endpoint key index missing: {rendered:?}"
        );
    }

    #[test]
    fn postgres_endpoint_keys_declaration_keeps_position_uniqueness_only() {
        let table = endpoint_api_keys::create_table_statement().to_string(PostgresQueryBuilder);
        assert!(!table.contains("FOREIGN KEY"));
        assert!(!table.contains("UNIQUE"));
        assert!(table.contains("clock_timestamp()"));
        let indexes = endpoint_api_keys::create_index_statements();
        assert_eq!(indexes.len(), 1);
        let index = indexes[0].to_string(PostgresQueryBuilder);
        assert!(index.contains("UNIQUE"));
        assert!(index.contains("(\"endpoint_id\", \"position\")"));
    }

    #[test]
    fn postgres_relay_declaration_keeps_applied_checks_and_unique_index() {
        let table = managed_relays::create_table_statement().to_owned();
        let sql = table.to_string(PostgresQueryBuilder);
        assert!(sql.contains("CHECK"), "relay checks missing: {sql}");
        assert!(sql.contains("relay_url"), "relay_url missing: {sql}");

        let indexes = managed_relays::create_index_statements();
        let unique = indexes
            .iter()
            .map(|index| index.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            unique.iter().any(
                |sql| sql.contains("managed_relays_relay_url_unique") && sql.contains("UNIQUE")
            ),
            "relay url unique index missing: {unique:?}"
        );
    }

    #[test]
    fn postgres_mcp_server_declaration_declares_named_checks_only() {
        let table = mcp_servers::create_table_statement().to_owned();
        let sql = table.to_string(PostgresQueryBuilder);
        for expected in [
            "ck_mcp_server_scope",
            "ck_mcp_server_owner",
            "ck_mcp_server_transport",
            "ck_mcp_server_auth_mode",
            "ck_mcp_server_provider_kind",
            "ck_mcp_server_lifecycle_policy",
            "ck_mcp_server_lifecycle_learned_mode",
            "ck_mcp_server_tool_filter_mode",
            "ck_mcp_servers_aggregate_naming_mode",
        ] {
            assert!(sql.contains(expected), "missing {expected}: {sql}");
        }
        assert!(
            !sql.contains("FOREIGN KEY"),
            "no foreign key allowed: {sql}"
        );
    }

    #[test]
    fn postgres_audit_declaration_keeps_applied_bigserial_identity() {
        let table = config_archive_audit::create_table_statement().to_owned();
        let sql = table.to_string(PostgresQueryBuilder);
        assert!(
            sql.to_ascii_lowercase().contains("bigserial"),
            "bigserial missing: {sql}"
        );
        assert!(
            !sql.contains("GENERATED BY DEFAULT AS IDENTITY"),
            "identity drifted: {sql}"
        );
    }

    #[test]
    fn sqlite_mcp_server_declaration_keeps_partial_unique_index() {
        let indexes = standalone_mcp_servers::create_index_statements();
        let rendered = indexes
            .iter()
            .map(|index| index.to_string(SqliteQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered.iter().any(
                |sql| sql.contains("idx_standalone_mcp_servers_source_endpoint")
                    && sql.contains("UNIQUE")
                    && sql.contains("WHERE")
            ),
            "partial unique index missing: {rendered:?}"
        );
    }

    #[test]
    fn postgres_user_declaration_keeps_bigserial_and_login_uniqueness() {
        let table = users::create_table_statement().to_owned();
        let sql = table.to_string(PostgresQueryBuilder);
        assert!(
            sql.to_ascii_lowercase().contains("bigserial"),
            "bigserial missing: {sql}"
        );
        assert!(
            sql.contains("login_name") && sql.contains("UNIQUE"),
            "login uniqueness missing: {sql}"
        );
    }

    #[test]
    fn postgres_client_key_declaration_keeps_bigserial_and_user_index() {
        let table = client_keys::create_table_statement().to_owned();
        let sql = table.to_string(PostgresQueryBuilder);
        assert!(
            sql.to_ascii_lowercase().contains("bigserial"),
            "bigserial missing: {sql}"
        );
        assert!(
            sql.contains("key_hash") && sql.contains("UNIQUE"),
            "key hash uniqueness missing: {sql}"
        );

        let indexes = client_keys::create_index_statements();
        let rendered = indexes
            .iter()
            .map(|index| index.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_client_keys_user")
                    && sql.contains("created_at")
                    && sql.contains("DESC")),
            "client key user index missing: {rendered:?}"
        );
    }

    #[test]
    fn sqlite_user_declaration_keeps_nocase_and_enabled_admin_index() {
        let table = standalone_users::create_table_statement().to_owned();
        let sql = table.to_string(SqliteQueryBuilder);
        assert!(sql.contains("COLLATE NOCASE"), "nocase missing: {sql}");
        assert!(sql.contains("CHECK"), "boolean checks missing: {sql}");
        assert!(
            sql.contains("CURRENT_TIMESTAMP"),
            "timestamp defaults missing: {sql}"
        );

        let indexes = standalone_users::create_index_statements();
        let rendered = indexes
            .iter()
            .map(|index| index.to_string(SqliteQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_standalone_users_enabled_admin")
                    && sql.contains("is_admin")
                    && sql.contains("login_name")),
            "enabled admin index missing: {rendered:?}"
        );
    }

    #[test]
    fn sqlite_client_key_declaration_keeps_envelope_columns_and_check() {
        let table = standalone_client_keys::create_table_statement().to_owned();
        let sql = table.to_string(SqliteQueryBuilder);
        let lowered = sql.to_ascii_lowercase();
        for expected in [
            "secret_ciphertext",
            "secret_nonce",
            "secret_key_version",
            "blob",
            "check",
        ] {
            assert!(lowered.contains(expected), "missing {expected}: {sql}");
        }
        assert!(
            !sql.contains("FOREIGN KEY"),
            "foreign keys stay in the migration: {sql}"
        );
        let key_id_start = sql.find("\"key_id\"").expect("key_id column");
        let key_id_rest = &sql[key_id_start..];
        let key_id_end = key_id_rest.find(',').unwrap_or(key_id_rest.len());
        let key_id_column = &key_id_rest[..key_id_end];
        assert!(
            !key_id_column.to_ascii_lowercase().contains("not null"),
            "key_id must keep the applied nullable TEXT PRIMARY KEY: {key_id_column}"
        );
    }

    #[test]
    fn postgres_model_route_declaration_keeps_applied_checks() {
        let rules = model_endpoint_rules::create_table_statement().to_owned();
        let rules_sql = rules.to_string(PostgresQueryBuilder);
        for expected in [
            "model_endpoint_rules_scope_check",
            "ck_model_route_owner",
            "ck_model_endpoint_rules_routing_strategy",
        ] {
            assert!(
                rules_sql.contains(expected),
                "missing {expected}: {rules_sql}"
            );
        }
        assert!(
            !rules_sql.contains("FOREIGN KEY"),
            "no foreign key allowed: {rules_sql}"
        );

        let targets = model_route_targets::create_table_statement().to_owned();
        let targets_sql = targets.to_string(PostgresQueryBuilder);
        for expected in [
            "ck_model_route_targets_native_api",
            "ck_model_route_targets_compact_mode",
            "ck_model_route_targets_thinking_effort",
            "ck_model_route_targets_embedding_dimensions_override",
        ] {
            assert!(
                targets_sql.contains(expected),
                "missing {expected}: {targets_sql}"
            );
        }
        assert!(
            !targets_sql.contains("FOREIGN KEY"),
            "no foreign key allowed: {targets_sql}"
        );
    }

    #[test]
    fn postgres_model_route_indexes_keep_rule_position_and_preference() {
        let rendered = model_route_targets::create_index_statements()
            .iter()
            .map(|index| index.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_model_route_targets_rule_position")
                    && sql.contains("UNIQUE")),
            "rule position unique index missing: {rendered:?}"
        );
        assert!(
            rendered.iter().any(|sql| {
                sql.contains("ux_model_route_targets_preferred_new_session")
                    && sql.contains("UNIQUE")
                    && sql.contains("WHERE")
            }),
            "preferred new session unique index missing: {rendered:?}"
        );
    }

    #[test]
    fn sqlite_model_route_declaration_matches_the_applied_standalone_schema() {
        let routes = standalone_model_routes::create_table_statement().to_owned();
        let routes_sql = routes.to_string(SqliteQueryBuilder);
        for expected in [
            "standalone_model_routes_scope_check",
            "standalone_model_routes_routing_strategy_check",
            "standalone_model_routes_enabled_check",
            "standalone_model_routes_owner_check",
        ] {
            assert!(
                routes_sql.contains(expected),
                "missing {expected}: {routes_sql}"
            );
        }
        // The applied table declares the routing strategy without a default
        // and accepts only the remaining strategy.
        assert!(
            routes_sql.contains("\"routing_strategy\" text NOT NULL,"),
            "routing strategy must have no default: {routes_sql}"
        );
        assert!(
            routes_sql.contains("\"routing_strategy\" IN ('responses_session_affinity')"),
            "routing strategy check drifted: {routes_sql}"
        );
        assert!(
            !routes_sql.contains("client_key_rendezvous"),
            "the removed strategy must not be declared: {routes_sql}"
        );
        // The applied scope, owner and pattern uniqueness is an unnamed table
        // constraint.
        assert!(
            routes_sql.contains("UNIQUE (\"scope\", \"owner_user_id\", \"model_pattern\")"),
            "scope, owner and pattern uniqueness missing: {routes_sql}"
        );

        let rendered = standalone_model_routes::create_index_statements()
            .iter()
            .map(|index| index.to_string(SqliteQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_standalone_routes_enabled")
                    && sql.contains("(\"scope\", \"owner_user_id\", \"enabled\")")),
            "applied route index missing: {rendered:?}"
        );

        let targets = standalone_model_route_targets::create_table_statement().to_owned();
        let targets_sql = targets.to_string(SqliteQueryBuilder);
        assert!(
            targets_sql.contains("UNIQUE (\"rule_id\", \"position\")"),
            "rule and position uniqueness missing: {targets_sql}"
        );
        assert!(
            targets_sql.contains(
                "typeof(\"standalone_model_route_targets\".\"embedding_dimensions_override\") = 'integer'"
            ),
            "dimension check drifted: {targets_sql}"
        );

        let rendered = standalone_model_route_targets::create_index_statements()
            .iter()
            .map(|index| index.to_string(SqliteQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered.iter().any(|sql| {
                sql.contains("ux_standalone_model_route_targets_preferred")
                    && sql.contains("UNIQUE")
                    && sql.contains("WHERE")
            }),
            "preferred new session unique index missing: {rendered:?}"
        );
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_standalone_route_targets_rule")
                    && sql.contains("(\"rule_id\", \"enabled\", \"position\")")),
            "applied target index missing: {rendered:?}"
        );
        assert!(
            !rendered
                .iter()
                .any(|sql| sql.contains("ux_standalone_model_route_targets_rule_position")),
            "an invented unique index name must not be declared: {rendered:?}"
        );
    }

    #[test]
    fn postgres_model_route_declaration_matches_the_applied_migrations() {
        let rules = model_endpoint_rules::create_table_statement().to_owned();
        let rules_sql = rules.to_string(PostgresQueryBuilder);
        // The applied column keeps its original default and the narrowed check.
        assert!(
            rules_sql
                .contains("\"routing_strategy\" text NOT NULL DEFAULT 'client_key_rendezvous'"),
            "routing strategy default drifted: {rules_sql}"
        );
        assert!(
            rules_sql.contains("\"routing_strategy\" = 'responses_session_affinity'"),
            "routing strategy check drifted: {rules_sql}"
        );
        let rendered = model_endpoint_rules::create_index_statements()
            .iter()
            .map(|index| index.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_model_endpoint_rules_scope_user")
                    && sql.contains("(\"scope\", \"owner_user_id\", \"enabled\", \"priority\")")),
            "applied rule index missing: {rendered:?}"
        );

        let targets = model_route_targets::create_table_statement().to_owned();
        let targets_sql = targets.to_string(PostgresQueryBuilder);
        for expected in [
            "\"native_api\" text NOT NULL DEFAULT 'auto'",
            "\"compact_mode\" text NOT NULL DEFAULT 'passthrough'",
            "\"dev_system_normalize\" bool NOT NULL DEFAULT (FALSE)",
            "\"prefer_new_sessions\" bool NOT NULL DEFAULT (FALSE)",
            "\"embedding_dimensions_override\" integer",
        ] {
            assert!(
                targets_sql.contains(expected),
                "missing {expected}: {targets_sql}"
            );
        }
        assert!(
            targets_sql.contains(
                "\"embedding_dimensions_override\" IS NULL OR \"embedding_dimensions_override\" > 0"
            ),
            "dimension check drifted: {targets_sql}"
        );

        let rendered = model_route_targets::create_index_statements()
            .iter()
            .map(|index| index.to_string(PostgresQueryBuilder))
            .collect::<Vec<_>>();
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_model_route_targets_rule_position")
                    && sql.contains("UNIQUE")),
            "rule position unique index missing: {rendered:?}"
        );
        assert!(
            rendered
                .iter()
                .any(|sql| sql.contains("idx_model_route_targets_endpoint_enabled")),
            "endpoint enabled index missing: {rendered:?}"
        );
        assert!(
            rendered.iter().any(|sql| {
                sql.contains("ux_model_route_targets_preferred_new_session")
                    && sql.contains("UNIQUE")
                    && sql.contains("WHERE")
            }),
            "preferred new session unique index missing: {rendered:?}"
        );
    }
}
