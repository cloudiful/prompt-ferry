mod approvals;
mod auth;
mod billing;
mod endpoints;
mod mcp;
mod me;
mod model_routes;
mod relay_input;
mod relay_secrets;
mod relay_validation;
mod relays;
mod server;
mod session_routing;
mod settings;
mod support;
mod usage;
mod usage_support;
mod users;

use std::net::SocketAddr;

use axum::routing::{get, patch, post};
pub(super) use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
pub(super) use chrono::NaiveDate;
pub(super) use serde_json::Value;
pub(super) use std::{
    collections::HashMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
pub(super) use uuid::Uuid;

pub(super) use crate::{
    config::{NativeApi, NativeApiSource},
    db::{self, EndpointCreate, UserCreate, UserUpdate},
    endpoint_protocol::endpoint_protocol_client,
    ip_acl,
    keys::{generate_client_key, hash_password, verify_password},
    llm_review::{
        ApprovalResolution, ApprovalStatus, LLM_REVIEW_SETTINGS_KEY, LlmReviewSettings,
        spawn_approval_webhook,
    },
    naming::{MODEL_ROUTE_TEST_ROUTING_KEY, MODEL_ROUTE_TEST_SESSION_KEY, SESSION_COOKIE_NAME},
    protocol::{BridgeMessage, ClientRoute, ConfigSnapshot, RelayIpPolicy},
    redact,
    routing::choose_preferred_target,
    usage_prompt::{REQUEST_CHAIN_DEPTH_LIMIT, RenderedPromptMessage, render_prompt_text},
    worker_admin_state::{
        bad_request, current_user, ensure_admin, error, internal, maybe_redact, new_session_id,
        session_id,
    },
    worker_admin_types::*,
};
use tower_http::cors::CorsLayer;

use self::{
    approvals::*, auth::*, billing::*, endpoints::*, mcp::*, model_routes::*, relays::*,
    session_routing::*, settings::*, usage::*, users::*,
};

pub(super) use self::support::*;
pub use self::{
    server::{router, run_admin_server},
    support::{publish_snapshot, set_bridge_sender},
};
pub use crate::worker_admin_state::AdminState;
