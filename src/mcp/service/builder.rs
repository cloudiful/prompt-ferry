//! The shared catalog builder.
//!
//! Every catalog reader composes its items from the same builder. The only
//! difference between the admin catalog and the runtime catalog is the
//! explicit visibility policy: the admin catalog keeps every upstream item,
//! the runtime catalog hides the disabled tools and resources.

use std::collections::HashMap;

use serde_json::Value;

use crate::db::McpServer;

use crate::mcp::cache::ServerCatalogSnapshot;
use crate::mcp::filtering::{is_disabled_item, is_tool_allowed};
use crate::mcp::output_schema::normalize_tool_output_schema;
use crate::mcp::protocol::{encode_resource_template_uri, encode_resource_uri};

pub(super) const MCP_AGGREGATE_NAMING_PASSTHROUGH_PREFERRED: &str = "passthrough_preferred";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AggregateKind {
    Tools,
    Prompts,
    Resources,
    Templates,
}

impl AggregateKind {
    pub(super) fn name_key(self) -> &'static str {
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

/// Which items one catalog reader may see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CatalogVisibility {
    /// The admin catalog keeps every upstream item, including the ones the
    /// runtime filters out.
    Admin,
    /// The runtime catalog hides the disabled tools and resources.
    Public,
}

#[derive(Clone, Debug)]
pub(super) struct AggregateCatalogEntry {
    pub(super) server: McpServer,
    pub(super) upstream_name: String,
    pub(super) qualified_name: String,
    pub(super) item: Value,
}

/// Build the catalog entries of one kind for the loaded servers.
pub(super) fn entries_for_kind(
    loaded: &[(McpServer, ServerCatalogSnapshot)],
    kind: AggregateKind,
    visibility: CatalogVisibility,
) -> Vec<AggregateCatalogEntry> {
    let mut entries = Vec::new();
    for (server, snapshot) in loaded {
        for item in kind.snapshot_items(snapshot) {
            let Some(upstream_name) = item.get(kind.name_key()).and_then(Value::as_str) else {
                continue;
            };
            if !is_visible(server, kind, upstream_name, visibility) {
                continue;
            }
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

fn is_visible(
    server: &McpServer,
    kind: AggregateKind,
    upstream_name: &str,
    visibility: CatalogVisibility,
) -> bool {
    if visibility == CatalogVisibility::Admin {
        return true;
    }
    match kind {
        AggregateKind::Tools => is_tool_allowed(server, upstream_name),
        AggregateKind::Resources | AggregateKind::Templates => {
            !is_disabled_item(server, "resources", upstream_name)
        }
        AggregateKind::Prompts => true,
    }
}

pub(super) fn raw_name_counts(entries: &[AggregateCatalogEntry]) -> HashMap<String, usize> {
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

pub(super) fn catalog_items_for_server(
    entries: &[AggregateCatalogEntry],
    server_name: &str,
    kind: AggregateKind,
) -> Vec<crate::worker_admin_types::McpCatalogItem> {
    let counts = raw_name_counts(entries);
    entries
        .iter()
        .filter(|entry| entry.server.name == server_name)
        .map(|entry| crate::worker_admin_types::McpCatalogItem {
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

/// Rewrite one entry into its aggregate form.
pub(super) fn aggregate_item(entry: AggregateCatalogEntry, kind: AggregateKind) -> Value {
    let mut item = entry.item;
    item[kind.name_key()] = Value::String(entry.qualified_name);
    if kind == AggregateKind::Tools {
        normalize_tool_output_schema(&mut item);
    }
    item
}
