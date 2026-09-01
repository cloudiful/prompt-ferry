use std::time::Duration;

use anyhow::Context;
use rmcp::model::ServerCapabilities;
use serde_json::{Value, json};

use crate::db::McpServer;

use super::ServerCatalogSnapshot;
use crate::mcp::transport;

pub async fn fetch_server_snapshot(
    pool: Option<&sqlx::PgPool>,
    server: &McpServer,
) -> anyhow::Result<ServerCatalogSnapshot> {
    let storage = pool.map(|pool| super::super::McpRuntimeStorage::postgres(pool.clone()));
    fetch_server_snapshot_with_storage(storage.as_ref(), server).await
}

pub(crate) async fn fetch_server_snapshot_with_storage(
    storage: Option<&super::super::McpRuntimeStorage>,
    server: &McpServer,
) -> anyhow::Result<ServerCatalogSnapshot> {
    let timeout = Duration::from_millis(server.timeout_ms.max(100) as u64);
    tokio::time::timeout(timeout, async {
        if server.transport == "builtin_minimax" {
            return Ok(crate::mcp::builtin::catalog());
        }
        let client = transport::connect(storage, server, None).await?;
        let result = async {
            let capabilities = client
                .peer()
                .peer_info()
                .map(|info| info.capabilities.clone())
                .unwrap_or_default();
            let probe_policy = catalog_probe_policy(server, &capabilities);

            let tools = if probe_policy.tools {
                read_peer_list(
                    &transport::peer_list_or_empty(client.peer().list_all_tools().await, "tools")?,
                    "tools",
                )
            } else {
                Vec::new()
            };
            let resources = if probe_policy.resources {
                read_peer_list(
                    &transport::peer_list_or_empty(
                        client.peer().list_all_resources().await,
                        "resources",
                    )?,
                    "resources",
                )
            } else {
                Vec::new()
            };
            let resource_templates = if probe_policy.resources {
                read_peer_list(
                    &transport::peer_list_or_empty(
                        client.peer().list_all_resource_templates().await,
                        "resourceTemplates",
                    )?,
                    "resourceTemplates",
                )
            } else {
                Vec::new()
            };
            let prompts = if probe_policy.prompts {
                read_peer_list(
                    &transport::peer_list_or_empty(
                        client.peer().list_all_prompts().await,
                        "prompts",
                    )?,
                    "prompts",
                )
            } else {
                Vec::new()
            };

            Ok(ServerCatalogSnapshot {
                tools,
                resources,
                resource_templates,
                prompts,
            })
        }
        .await;
        let cancel_result = client.cancel().await.context("failed to close mcp client");
        match (result, cancel_result) {
            (Ok(snapshot), Ok(_)) => Ok(snapshot),
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err),
        }
    })
    .await?
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CatalogProbePolicy {
    tools: bool,
    resources: bool,
    prompts: bool,
}

fn catalog_probe_policy(
    server: &McpServer,
    capabilities: &ServerCapabilities,
) -> CatalogProbePolicy {
    let probe_all = server.transport == "http" && capabilities_unspecified(capabilities);
    CatalogProbePolicy {
        tools: probe_all || capabilities.tools.is_some(),
        resources: probe_all || capabilities.resources.is_some(),
        prompts: probe_all || capabilities.prompts.is_some(),
    }
}

fn capabilities_unspecified(capabilities: &ServerCapabilities) -> bool {
    capabilities.tools.is_none()
        && capabilities.resources.is_none()
        && capabilities.prompts.is_none()
}

fn read_peer_list(response: &Value, result_key: &str) -> Vec<Value> {
    list_result_items(response, result_key)
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn list_result_items<'a>(
    response: &'a Value,
    result_key: &str,
) -> Option<&'a Vec<Value>> {
    response
        .pointer(&format!("/result/{result_key}"))
        .and_then(Value::as_array)
        .or_else(|| response.get("result").and_then(Value::as_array))
        .or_else(|| response.get(result_key).and_then(Value::as_array))
}

pub fn snapshot_to_test_values(snapshot: &ServerCatalogSnapshot) -> (Value, Value, Value) {
    (
        json!({ "tools": snapshot.tools.clone() }),
        json!({ "resources": snapshot.resources.clone() }),
        json!({ "prompts": snapshot.prompts.clone() }),
    )
}
#[cfg(test)]
mod tests {
    use chrono::Utc;
    use rmcp::model::ServerCapabilities;

    use super::*;

    fn server(transport: &str) -> McpServer {
        McpServer {
            server_id: uuid::Uuid::new_v4(),
            source_endpoint_id: None,
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "alpha".to_string(),
            aggregate_naming_mode: "qualified_only".to_string(),
            transport: transport.to_string(),
            url: Some("http://127.0.0.1:3000/mcp".to_string()),
            command: None,
            args: json!([]),
            env_json: json!({}),
            bearer_tokens_json: json!([]),
            http_headers_json: json!({}),
            auth_mode: "none".to_string(),
            basic_username: None,
            basic_password: None,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: json!([]),
            disabled_tools: json!([]),
            disabled_resources: json!([]),
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "auto".to_string(),
            lifecycle_manual_protocol_version: None,
            lifecycle_learned_mode: None,
            lifecycle_learned_protocol_version: None,
            lifecycle_learned_for_updated_at: None,
            lifecycle_learned_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn http_servers_probe_catalog_when_capabilities_are_unspecified() {
        let policy = catalog_probe_policy(&server("http"), &ServerCapabilities::default());

        assert_eq!(
            policy,
            CatalogProbePolicy {
                tools: true,
                resources: true,
                prompts: true,
            }
        );
    }

    #[test]
    fn stdio_servers_respect_unspecified_capabilities() {
        let policy = catalog_probe_policy(&server("stdio"), &ServerCapabilities::default());

        assert_eq!(
            policy,
            CatalogProbePolicy {
                tools: false,
                resources: false,
                prompts: false,
            }
        );
    }

    #[test]
    fn explicit_capabilities_are_respected_without_forcing_all_lists() {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        let policy = catalog_probe_policy(&server("http"), &capabilities);

        assert_eq!(
            policy,
            CatalogProbePolicy {
                tools: true,
                resources: false,
                prompts: false,
            }
        );
    }
}
