mod auth;
mod init;
mod node;
// Issue #599 R2c: the ChatGPT (Codex) private-interface mapping layer is
// public so the runtime request path can reach the same helpers the admin
// handlers use (the pattern already used by `pub mod oauth`).
pub mod chatgpt_backend;
// Issue #759 P1: strict ChatGPT quota-window parsing, split out to keep
// `chatgpt_backend` bounded.
mod chatgpt_quota_windows;
// Issue #599 R2f.1: access-token claim extraction and the request-context
// header helper shared by the ChatGPT mapping layer, split out to keep
// `chatgpt_backend` bounded.
mod codex_claims;
mod codex_request_headers;
mod config_audit;
mod config_export;
mod config_import;
mod endpoint_usage;
mod endpoints;
mod mcp;
mod mcp_oauth;
mod me;
mod me_catalog;
mod model_routes;
mod nodes;
pub mod oauth;
mod oauth_client;
mod oauth_session;
pub(crate) mod openapi_paths;
mod quota_inference;
mod quota_key_observations;
mod server;
mod session_routing;
mod settings;
mod settings_cache_alert;
mod support;
mod usage;
mod usage_support;
mod users;

use axum::routing::{get, patch, post};
pub(super) use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
pub(super) use chrono::{NaiveDate, Utc};
pub(super) use serde_json::Value;
pub(super) use std::{
    collections::HashMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
pub(super) use uuid::Uuid;

pub(super) use crate::{
    authz::{ControlAction, UserAction},
    config::{NativeApi, NativeApiSource},
    db::{self, EndpointCreate, UserCreate, UserUpdate},
    ip_acl,
    keys::{hash_password, verify_password},
    naming::{MODEL_ROUTE_TEST_SESSION_KEY, SESSION_COOKIE_NAME},
    protocol::{BridgeMessage, ConfigSnapshot, RelayIpPolicy},
    redact,
    routing::choose_preferred_target,
    usage_prompt::{REQUEST_CHAIN_DEPTH_LIMIT, RenderedPromptMessage, render_prompt_text},
    worker_admin::redaction_rule_timestamps::stamp_custom_string_timestamps,
    worker_admin_state::{
        ApiError, bad_request, current_user, ensure_admin, ensure_user, error, internal,
        maybe_redact, new_session_id, session_id,
    },
    worker_admin_types::*,
};
use tower_http::cors::CorsLayer;

use self::{
    auth::*, config_audit::*, config_export::*, config_import::*, endpoint_usage::*, endpoints::*,
    init::*, mcp::*, mcp_oauth::*, model_routes::*, node::*, nodes::*, quota_inference::*,
    quota_key_observations::*, session_routing::*, settings::*, settings_cache_alert::*, usage::*,
    users::*,
};

pub(super) use self::support::*;
pub use self::{
    server::router,
    support::{publish_snapshot, publish_snapshot_if_changed, set_bridge_sender},
};
pub use crate::worker_admin_state::AdminState;
