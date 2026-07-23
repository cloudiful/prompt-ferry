use chrono::Utc;
use serde_json::json;

use super::*;

fn server(name: &str) -> McpServer {
    McpServer {
        server_id: uuid::Uuid::new_v4(),
        scope: "admin".to_string(),
        owner_user_id: None,
        name: name.to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "http".to_string(),
        url: Some("http://127.0.0.1:3000/mcp".to_string()),
        command: None,
        args: json!([]),
        env_json: json!({}),
        bearer_tokens_json: json!([]),
        http_headers_json: json!({}),
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: json!([]),
        disabled_tools: json!([]),
        disabled_resources: json!([]),
        daily_max_requests: None,
        monthly_max_requests: None,
        enabled: true,
        timeout_ms: 30_000,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn aggregate_reads_cached_servers_only() {
    let cache = McpCatalogCache::new();
    let cached = server("alpha");
    let missing = server("beta");
    cache
        .put(
            &cached,
            ServerCatalogSnapshot {
                tools: vec![json!({"name": "cached_tool"})],
                resources: Vec::new(),
                prompts: Vec::new(),
            },
        )
        .await;

    let tools = aggregate_tools(&cache, &[cached, missing]).await.unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "alpha__cached_tool");
}

#[tokio::test]
async fn server_catalog_requires_warm_cache() {
    let cache = McpCatalogCache::new();
    let server = server("alpha");

    let error = catalog_for_server(&cache, &[server], "alpha")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("catalog is not ready"));
}

#[tokio::test]
async fn server_catalog_keeps_aggregate_name_conflicts() {
    let cache = McpCatalogCache::new();
    let mut alpha = server("alpha");
    alpha.aggregate_naming_mode = "passthrough_preferred".to_string();
    let mut beta = server("beta");
    beta.aggregate_naming_mode = "passthrough_preferred".to_string();
    let snapshot = ServerCatalogSnapshot {
        tools: vec![json!({"name": "shared_tool"})],
        resources: Vec::new(),
        prompts: Vec::new(),
    };
    cache.put(&alpha, snapshot.clone()).await;
    cache.put(&beta, snapshot).await;

    let catalog = catalog_for_server(&cache, &[alpha, beta], "alpha")
        .await
        .unwrap();

    assert_eq!(catalog.tools[0].aggregate_names, vec!["alpha__shared_tool"]);
}
