use chrono::Utc;
use serde_json::json;

use super::*;

fn server(name: &str) -> McpServer {
    McpServer {
        server_id: uuid::Uuid::new_v4(),
        source_endpoint_id: None,
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
                resource_templates: Vec::new(),
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
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    cache.put(&alpha, snapshot.clone()).await;
    cache.put(&beta, snapshot).await;

    let catalog = catalog_for_server(&cache, &[alpha, beta], "alpha")
        .await
        .unwrap();

    assert_eq!(catalog.tools[0].aggregate_names, vec!["alpha__shared_tool"]);
}

#[tokio::test]
async fn catalog_exposes_disabled_tool_while_aggregate_filters_it() {
    let cache = McpCatalogCache::new();
    let mut server_cfg = server("alpha");
    server_cfg.disabled_tools = json!(["secret_tool"]);
    server_cfg.disabled_resources = json!(["file:///secret"]);
    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "public_tool", "description": "pub"}),
            json!({"name": "secret_tool", "description": "sec"}),
        ],
        resources: vec![
            json!({"uri": "file:///public", "name": "pub"}),
            json!({"uri": "file:///secret", "name": "sec"}),
        ],
        resource_templates: vec![
            json!({"uriTemplate": "git://{owner}/{repo}/issues"}),
            json!({"uriTemplate": "git://{owner}/{repo}/secret"}),
        ],
        prompts: vec![json!({"name": "prompt_a"})],
    };
    // Mark secret template as disabled via disabled_resources
    server_cfg.disabled_resources = json!(["file:///secret", "git://{owner}/{repo}/secret"]);
    cache.put(&server_cfg, snapshot.clone()).await;

    // Cache retains unfiltered snapshot
    let cached = cache.get(&server_cfg).await.unwrap();
    assert_eq!(cached.tools.len(), 2);
    assert_eq!(cached.resources.len(), 2);
    assert_eq!(cached.resource_templates.len(), 2);

    // Admin catalog must expose disabled items
    let catalog = catalog_for_server(&cache, &[server_cfg.clone()], "alpha")
        .await
        .unwrap();
    assert_eq!(catalog.tools.len(), 2);
    assert!(catalog.tools.iter().any(|t| t.name == "secret_tool"));
    assert_eq!(catalog.resources.len(), 2);
    assert!(catalog.resources.iter().any(|r| r.name == "file:///secret"));

    // Runtime aggregate must filter disabled items per server
    let agg_tools = aggregate_tools(&cache, &[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg_tools.len(), 1);
    assert_eq!(agg_tools[0]["name"], "alpha__public_tool");

    let agg_resources = aggregate_resources(&cache, &[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg_resources.len(), 1);
    assert_eq!(
        agg_resources[0]["uri"],
        "mcp://alpha/file%3A%2F%2F%2Fpublic"
    );

    let agg_templates = aggregate_resource_templates(&cache, &[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg_templates.len(), 1);
    assert!(
        agg_templates[0]["uriTemplate"]
            .as_str()
            .unwrap()
            .contains("issues")
    );
}

#[tokio::test]
async fn whitelist_catalog_shows_all_while_aggregate_shows_allowed_only() {
    let cache = McpCatalogCache::new();
    let mut server_cfg = server("alpha");
    server_cfg.tool_filter_mode = "whitelist".to_string();
    server_cfg.allowed_tools = json!(["allowed_tool"]);
    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "allowed_tool"}),
            json!({"name": "other_tool"}),
            json!({"name": "third_tool"}),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    cache.put(&server_cfg, snapshot.clone()).await;

    // Admin catalog shows all upstream tools
    let catalog = catalog_for_server(&cache, &[server_cfg.clone()], "alpha")
        .await
        .unwrap();
    assert_eq!(catalog.tools.len(), 3);
    assert!(catalog.tools.iter().any(|t| t.name == "other_tool"));

    // Aggregate shows only whitelisted
    let agg = aggregate_tools(&cache, &[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg.len(), 1);
    assert_eq!(agg[0]["name"], "alpha__allowed_tool");

    // Underlying cache still retains all
    let cached = cache.get(&server_cfg).await.unwrap();
    assert_eq!(cached.tools.len(), 3);
}

#[tokio::test]
async fn aggregate_filters_per_server_independently() {
    let cache = McpCatalogCache::new();
    let mut alpha = server("alpha");
    alpha.disabled_tools = json!(["shared_tool"]);
    let mut beta = server("beta");
    beta.disabled_tools = json!([]);

    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "shared_tool"}),
            json!({"name": "unique_alpha"}),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    let beta_snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "shared_tool"}),
            json!({"name": "unique_beta"}),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    cache.put(&alpha, snapshot).await;
    cache.put(&beta, beta_snapshot).await;

    let agg = aggregate_tools(&cache, &[alpha, beta]).await.unwrap();
    // alpha's shared_tool is disabled, so only beta's shared_tool and both uniques remain
    assert_eq!(agg.len(), 3);
    let names: Vec<_> = agg
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.contains(&"alpha__shared_tool".to_string()));
    assert!(names.contains(&"beta__shared_tool".to_string()));
    assert!(names.contains(&"alpha__unique_alpha".to_string()));
    assert!(names.contains(&"beta__unique_beta".to_string()));

    // Catalog for alpha still shows its disabled tool
    // Need to reload cache for catalog check: create fresh cache with both servers
    let cache2 = McpCatalogCache::new();
    let mut alpha2 = server("alpha");
    alpha2.disabled_tools = json!(["shared_tool"]);
    let mut beta2 = server("beta");
    beta2.disabled_tools = json!([]);
    cache2
        .put(
            &alpha2,
            ServerCatalogSnapshot {
                tools: vec![
                    json!({"name": "shared_tool"}),
                    json!({"name": "unique_alpha"}),
                ],
                resources: Vec::new(),
                resource_templates: Vec::new(),
                prompts: Vec::new(),
            },
        )
        .await;
    cache2
        .put(
            &beta2,
            ServerCatalogSnapshot {
                tools: vec![json!({"name": "shared_tool"})],
                resources: Vec::new(),
                resource_templates: Vec::new(),
                prompts: Vec::new(),
            },
        )
        .await;
    let catalog_alpha = catalog_for_server(&cache2, &[alpha2.clone(), beta2.clone()], "alpha")
        .await
        .unwrap();
    assert!(catalog_alpha.tools.iter().any(|t| t.name == "shared_tool"));
}

#[tokio::test]
async fn prompts_remain_unfiltered_in_both_catalog_and_aggregate() {
    let cache = McpCatalogCache::new();
    let mut server_cfg = server("alpha");
    server_cfg.disabled_tools = json!(["any"]);
    let snapshot = ServerCatalogSnapshot {
        tools: vec![json!({"name": "any"})],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: vec![json!({"name": "prompt_one"}), json!({"name": "prompt_two"})],
    };
    cache.put(&server_cfg, snapshot).await;

    let catalog = catalog_for_server(&cache, &[server_cfg.clone()], "alpha")
        .await
        .unwrap();
    assert_eq!(catalog.prompts.len(), 2);

    let agg = aggregate_prompts(&cache, &[server_cfg]).await.unwrap();
    assert_eq!(agg.len(), 2);
}

#[tokio::test]
async fn named_server_filtering_retains_cache_while_hiding_disabled() {
    // Mirrors ProxyService::cached_server_list filtering: tools via is_tool_allowed,
    // resources/templates via disabled_resources, while McpCatalogCache retains all.
    use crate::mcp::filtering::{is_disabled_item, is_tool_allowed};

    let cache = McpCatalogCache::new();
    let mut server_cfg = server("alpha");
    server_cfg.disabled_tools = json!(["secret_tool"]);
    server_cfg.disabled_resources = json!(["file:///secret", "git://{a}/{b}/secret"]);
    server_cfg.tool_filter_mode = "blacklist".to_string();

    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "public_tool"}),
            json!({"name": "secret_tool"}),
        ],
        resources: vec![
            json!({"uri": "file:///public"}),
            json!({"uri": "file:///secret"}),
        ],
        resource_templates: vec![
            json!({"uriTemplate": "git://{a}/{b}/public"}),
            json!({"uriTemplate": "git://{a}/{b}/secret"}),
        ],
        prompts: vec![json!({"name": "p1"})],
    };
    cache.put(&server_cfg, snapshot.clone()).await;

    // Cache must retain everything
    let cached = cache.get(&server_cfg).await.unwrap();
    assert_eq!(cached.tools.len(), 2);
    assert_eq!(cached.resources.len(), 2);
    assert_eq!(cached.resource_templates.len(), 2);

    // Simulate named-server list filtering (as done in server/ops.rs)
    let filtered_tools: Vec<_> = cached
        .tools
        .iter()
        .filter(|item| {
            item.get("name")
                .and_then(|v| v.as_str())
                .is_none_or(|n| is_tool_allowed(&server_cfg, n))
        })
        .collect();
    assert_eq!(filtered_tools.len(), 1);
    assert_eq!(filtered_tools[0]["name"], "public_tool");

    let filtered_resources: Vec<_> = cached
        .resources
        .iter()
        .filter(|item| {
            item.get("uri")
                .and_then(|v| v.as_str())
                .is_none_or(|u| !is_disabled_item(&server_cfg, "resources", u))
        })
        .collect();
    assert_eq!(filtered_resources.len(), 1);
    assert_eq!(filtered_resources[0]["uri"], "file:///public");

    let filtered_templates: Vec<_> = cached
        .resource_templates
        .iter()
        .filter(|item| {
            item.get("uriTemplate")
                .and_then(|v| v.as_str())
                .is_none_or(|u| !is_disabled_item(&server_cfg, "resources", u))
        })
        .collect();
    assert_eq!(filtered_templates.len(), 1);
    assert!(
        filtered_templates[0]["uriTemplate"]
            .as_str()
            .unwrap()
            .contains("public")
    );

    // Admin catalog still shows all
    let catalog = catalog_for_server(&cache, &[server_cfg.clone()], "alpha")
        .await
        .unwrap();
    assert_eq!(catalog.tools.len(), 2);
    assert_eq!(catalog.resources.len(), 2);
}
