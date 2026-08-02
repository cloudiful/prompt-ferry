mod selection;
mod session_affinity;
#[cfg(test)]
mod session_affinity_recovery_tests;
#[cfg(test)]
mod session_affinity_tests;

#[cfg(test)]
pub(in crate::worker::runtime) use selection::rendezvous_target;
pub(in crate::worker::runtime) use selection::{
    clear_invalid_conversation_endpoint_key_override, discover_dynamic_model_route,
    materialize_route_api_key_selection, select_route_for_candidate,
};
pub(in crate::worker::runtime) use session_affinity::RouteAffinityError;

pub(in crate::worker::runtime) fn upstream_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}
