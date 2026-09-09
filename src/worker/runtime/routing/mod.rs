#[cfg(test)]
mod quota_opencode_go_tests;
#[cfg(test)]
mod quota_openrouter_tests;
mod quota_selection;
#[cfg(test)]
mod quota_selection_tests;
mod selection;
mod session_affinity;
#[cfg(test)]
mod session_affinity_opencode_go_tests;
#[cfg(test)]
mod session_affinity_openrouter_tests;
mod session_affinity_quota;
#[cfg(test)]
mod session_affinity_quota_tests;
#[cfg(test)]
mod session_affinity_recovery_tests;
#[cfg(test)]
mod session_affinity_tests;

#[cfg(test)]
pub(in crate::worker::runtime) use selection::materialize_route_api_key_selection;
#[cfg(test)]
pub(in crate::worker::runtime) use selection::rendezvous_target;
pub(in crate::worker::runtime) use selection::{
    clear_invalid_conversation_endpoint_key_override, discover_dynamic_model_route,
    materialize_route_api_key_selection_with_quota, select_route_for_candidate,
};
pub(in crate::worker::runtime) use session_affinity::RouteAffinityError;

/// Plain base+path join used by tests and the legacy plain-JSON callers.
/// The runtime HTTP path goes through `worker::runtime::ai::upstream::
/// upstream_url_for_route` (and the P4 minimum-field sibling
/// `upstream_url_for_route_parts`), which apply the GLM `/v1` strip and
/// the MiniMax `/anthropic` prefix remap. This helper remains for tests
/// that only need to assert the joiner behavior in isolation.
pub(in crate::worker::runtime) fn upstream_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}
