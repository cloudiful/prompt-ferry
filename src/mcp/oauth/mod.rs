//! Personal MCP OAuth backend (issue #1208 P3).
//!
//! A personal (user-scoped) HTTP MCP server can authenticate with OAuth. The
//! flow is owner-bound end to end: only the owner may start, complete, refresh
//! or disconnect it, the owner must still be active, and the authorization
//! state is single-use and bound to the browser session that started it.
//!
//! The module is a thin layer over `rmcp`'s public authorization API. All
//! persistence, encryption, configuration fingerprinting and refresh fencing
//! live in the frozen P2 storage boundary; all outbound HTTP goes through the
//! pinned client in [`http`].

mod flow;
pub(crate) mod http;
mod store;
#[cfg(test)]
mod tests;

pub(crate) use flow::{McpOAuthError, McpOAuthService, McpOAuthState};

/// The only scope requested for, and accepted from, a personal MCP OAuth
/// grant. Write scopes are never requested.
pub(crate) const MCP_OAUTH_SCOPE: &str = "read-mcp";
