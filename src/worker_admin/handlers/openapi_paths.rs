//! Generated `utoipa::path` metadata for the admin API, re-exported under the
//! operation tokens consumed by the `OpenApi` derive in
//! `openapi_export::doc_groups`.
//!
//! Each alias points at the real handler's generated `__path_<fn>` item; no
//! documentation stub remains. The aliases exist because a few handler
//! functions share a name across modules (the `me` and `users` client-key
//! handlers) and because the documented operation names are not the Rust
//! function names.

pub(crate) use super::auth::__path_login as __path_auth_login;
pub(crate) use super::auth::__path_logout as __path_auth_logout;
pub(crate) use super::auth::__path_me as __path_auth_me;
pub(crate) use super::config_audit::__path_list_config_audit;
pub(crate) use super::config_export::{__path_config_export_metadata, __path_export_config};
pub(crate) use super::config_import::{__path_import_config, __path_preview_config_import};
pub(crate) use super::endpoint_usage::{__path_organization_usage, __path_token_plan_usage};
pub(crate) use super::endpoints::{
    __path_create_endpoint, __path_delete_endpoint,
    __path_list_endpoint_models as __path_endpoint_models, __path_list_endpoints,
    __path_test_endpoint, __path_update_endpoint,
};
pub(crate) use super::init::{__path_setup_initialize, __path_setup_status};
pub(crate) use super::mcp::{
    __path_create_mcp_server, __path_delete_mcp_server, __path_get_mcp_catalog,
    __path_list_mcp_providers, __path_list_mcp_servers, __path_test_mcp_server,
    __path_update_mcp_server,
};
pub(crate) use super::mcp_oauth::{
    __path_disconnect_mcp_oauth, __path_get_mcp_oauth_status, __path_mcp_oauth_callback,
    __path_refresh_mcp_oauth, __path_start_mcp_oauth,
};
pub(crate) use super::me::{
    __path_create_client_key as __path_me_create_client_key,
    __path_delete_client_key as __path_me_delete_client_key,
    __path_list_available_models as __path_me_list_models,
    __path_list_client_keys as __path_me_list_client_keys,
    __path_update_client_key as __path_me_update_client_key,
};
pub(crate) use super::model_routes::{
    __path_create_model_route, __path_delete_model_route, __path_list_model_routes,
    __path_test_model_route, __path_update_model_route,
};
pub(crate) use super::node::__path_node_status;
pub(crate) use super::nodes::__path_list_nodes;
pub(crate) use super::oauth::{
    __path_oauth_browser_complete, __path_oauth_browser_start, __path_oauth_clear,
    __path_oauth_device_poll, __path_oauth_device_start, __path_oauth_refresh, __path_oauth_status,
};
pub(crate) use super::quota_inference::__path_quota_inference;
pub(crate) use super::quota_key_observations::{
    __path_quota_key_observation_history, __path_quota_key_observation_trend,
};
pub(crate) use super::session_routing::{
    __path_delete_conversation_endpoint_override, __path_get_conversation_endpoint_override,
    __path_set_conversation_endpoint_override,
    __path_usage_event_session_affinity_reset as __path_request_record_reset_session_affinity,
    __path_usage_event_session_route_options as __path_request_record_session_route_options,
};
pub(crate) use super::settings::{
    __path_get_endpoint_setting, __path_get_model_route_whitelist, __path_get_redaction_setting,
    __path_get_relay_ip_whitelist, __path_get_request_content_logging, __path_get_shared_valkey,
    __path_get_stream_delta_batching, __path_get_usage_retention,
    __path_list_redaction_custom_strings, __path_preview_redaction, __path_set_endpoint_setting,
    __path_set_model_route_whitelist, __path_set_redaction_setting, __path_set_relay_ip_whitelist,
    __path_set_request_content_logging, __path_set_shared_valkey, __path_set_stream_delta_batching,
    __path_set_usage_retention,
};
pub(crate) use super::settings_cache_alert::{
    __path_get_cache_alert_setting, __path_set_cache_alert_setting,
};
pub(crate) use super::usage::__path_bridge_status;
pub(crate) use super::usage::{
    __path_clear_usage_events as __path_clear_request_records,
    __path_prune_usage_events as __path_prune_request_records,
    __path_usage_event_detail as __path_request_record_detail,
    __path_usage_events as __path_list_request_records,
    __path_usage_facets as __path_request_record_facets,
    __path_usage_overview as __path_request_record_overview,
    __path_usage_request_full as __path_request_record_full,
    __path_usage_series as __path_request_record_series,
    __path_usage_summary as __path_request_record_summary,
};
pub(crate) use super::users::{
    __path_create_client_key, __path_create_user, __path_delete_client_key, __path_delete_user,
    __path_list_client_keys, __path_list_user_options, __path_list_users, __path_reset_password,
    __path_update_client_key, __path_update_user,
};
