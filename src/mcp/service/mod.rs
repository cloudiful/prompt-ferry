use std::collections::HashMap;

use serde_json::Value;
use tracing::warn;

use crate::{
    db::McpServer,
    worker_admin_types::{McpCatalogItem, McpCatalogResponse},
};

use super::{
    cache::{McpCatalogCache, ServerCatalogSnapshot},
    protocol::{encode_resource_template_uri, encode_resource_uri},
};

mod catalog;
mod snapshot;
#[cfg(test)]
mod tests;

pub use catalog::McpCatalogService;
pub use snapshot::{fetch_server_snapshot, snapshot_to_test_values};

const MCP_AGGREGATE_NAMING_PASSTHROUGH_PREFERRED: &str = "passthrough_preferred";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AggregateKind {
    Tools,
    Prompts,
    Resources,
    Templates,
}

impl AggregateKind {
    fn name_key(self) -> &'static str {
        match self {
            Self::Tools | Self::Prompts => "name",
            Self::Resources => "uri",
            Self::Templates => "uriTemplate",
        }
    }

    fn alias_for(self, server_name: &str, upstream_name: &str) -> String {
        match self {
            Self::Tools | Self::Prompts => format!("{server_name}__{upstream_name}"),
            Self::Resources => encode_resource_uri(server_name, upstream_name),
            Self::Templates => encode_resource_template_uri(server_name, upstream_name),
        }
    }

    fn snapshot_items(self, snapshot: &ServerCatalogSnapshot) -> &[Value] {
        match self {
            Self::Tools => &snapshot.tools,
            Self::Prompts => &snapshot.prompts,
            Self::Resources => &snapshot.resources,
            Self::Templates => &snapshot.resource_templates,
        }
    }
}

#[derive(Clone, Debug)]
struct AggregateCatalogEntry {
    server: McpServer,
    upstream_name: String,
    qualified_name: String,
    item: Value,
}

pub async fn catalog_for_server(
    cache: &McpCatalogCache,
    servers: &[McpServer],
    server_name: &str,
) -> anyhow::Result<McpCatalogResponse> {
    if !servers.iter().any(|server| server.name == server_name) {
        return Err(anyhow::anyhow!("mcp server not found or disabled"));
    }
    let mut loaded = Vec::with_capacity(servers.len());
    for server in servers {
        if let Some(snapshot) = cache.get(server).await {
            loaded.push((server.clone(), snapshot));
        }
    }
    if !loaded.iter().any(|(server, _)| server.name == server_name) {
        return Err(anyhow::anyhow!("mcp catalog is not ready"));
    }
    let tools = entries_for_kind(&loaded, AggregateKind::Tools);
    let resources = entries_for_kind(&loaded, AggregateKind::Resources);
    let prompts = entries_for_kind(&loaded, AggregateKind::Prompts);

    Ok(McpCatalogResponse {
        tools: catalog_items_for_server(&tools, server_name, AggregateKind::Tools),
        resources: catalog_items_for_server(&resources, server_name, AggregateKind::Resources),
        prompts: catalog_items_for_server(&prompts, server_name, AggregateKind::Prompts),
    })
}

pub async fn aggregate_tools(
    cache: &McpCatalogCache,
    servers: &[McpServer],
) -> anyhow::Result<Vec<Value>> {
    aggregate_prefixed_items(cache, servers, AggregateKind::Tools).await
}

pub async fn aggregate_resources(
    cache: &McpCatalogCache,
    servers: &[McpServer],
) -> anyhow::Result<Vec<Value>> {
    aggregate_prefixed_items(cache, servers, AggregateKind::Resources).await
}

pub async fn aggregate_resource_templates(
    cache: &McpCatalogCache,
    servers: &[McpServer],
) -> anyhow::Result<Vec<Value>> {
    aggregate_prefixed_items(cache, servers, AggregateKind::Templates).await
}

pub async fn aggregate_prompts(
    cache: &McpCatalogCache,
    servers: &[McpServer],
) -> anyhow::Result<Vec<Value>> {
    aggregate_prefixed_items(cache, servers, AggregateKind::Prompts).await
}

async fn aggregate_prefixed_items(
    cache: &McpCatalogCache,
    servers: &[McpServer],
    kind: AggregateKind,
) -> anyhow::Result<Vec<Value>> {
    let mut loaded = Vec::with_capacity(servers.len());
    for server in servers {
        if let Some(snapshot) = cache.get(server).await {
            loaded.push((server.clone(), snapshot));
        } else {
            warn!(
                category = "mcp_catalog_cache",
                server_name = %server.name,
                "skipping MCP server without a cached catalog"
            );
        }
    }

    Ok(entries_for_kind(&loaded, kind)
        .into_iter()
        .map(|entry| {
            let mut item = entry.item;
            item[kind.name_key()] = Value::String(entry.qualified_name);
            item
        })
        .collect())
}

fn entries_for_kind(
    loaded: &[(McpServer, ServerCatalogSnapshot)],
    kind: AggregateKind,
) -> Vec<AggregateCatalogEntry> {
    let mut entries = Vec::new();
    for (server, snapshot) in loaded {
        for item in kind.snapshot_items(snapshot) {
            let Some(upstream_name) = item.get(kind.name_key()).and_then(Value::as_str) else {
                continue;
            };
            entries.push(AggregateCatalogEntry {
                server: server.clone(),
                upstream_name: upstream_name.to_string(),
                qualified_name: kind.alias_for(&server.name, upstream_name),
                item: item.clone(),
            });
        }
    }
    entries
}

fn raw_name_counts(entries: &[AggregateCatalogEntry]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for entry in entries {
        if entry.server.aggregate_naming_mode == MCP_AGGREGATE_NAMING_PASSTHROUGH_PREFERRED {
            *counts.entry(entry.upstream_name.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn visible_identifier(entry: &AggregateCatalogEntry, counts: &HashMap<String, usize>) -> String {
    if entry.server.aggregate_naming_mode == MCP_AGGREGATE_NAMING_PASSTHROUGH_PREFERRED
        && counts
            .get(&entry.upstream_name)
            .copied()
            .unwrap_or_default()
            == 1
    {
        return entry.upstream_name.clone();
    }
    entry.qualified_name.clone()
}

fn aggregate_names(entry: &AggregateCatalogEntry, counts: &HashMap<String, usize>) -> Vec<String> {
    let visible_name = visible_identifier(entry, counts);
    if visible_name == entry.qualified_name {
        return vec![entry.qualified_name.clone()];
    }
    vec![visible_name, entry.qualified_name.clone()]
}

fn catalog_items_for_server(
    entries: &[AggregateCatalogEntry],
    server_name: &str,
    kind: AggregateKind,
) -> Vec<McpCatalogItem> {
    let counts = raw_name_counts(entries);
    entries
        .iter()
        .filter(|entry| entry.server.name == server_name)
        .map(|entry| McpCatalogItem {
            name: entry.upstream_name.clone(),
            aggregate_names: aggregate_names(entry, &counts),
            title: entry
                .item
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
            description: entry
                .item
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
        .map(|mut item| {
            if kind == AggregateKind::Resources && item.aggregate_names.is_empty() {
                item.aggregate_names.push(item.name.clone());
            }
            item
        })
        .collect()
}
