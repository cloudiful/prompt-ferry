// The modules below are `#[utoipa::path]` stubs: they exist only so utoipa can
// emit the generated `__path_*` items consumed by the `OpenApi` derives. The
// anchor functions themselves are never called, so dead-code analysis is
// intentionally relaxed for this scaffolding-only subtree.
#![allow(dead_code)]

mod approvals;
mod auth;
mod billing;
mod bridge;
mod doc;
mod doc_groups;
mod endpoints;
mod mcp;
mod me;
mod model_routes;
mod relays;
mod schemas;
mod settings;
mod usage;
mod users;

pub use doc::export_admin_api;
