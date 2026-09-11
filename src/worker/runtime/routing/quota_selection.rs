use crate::{
    db,
    worker::runtime::{context::RuntimeServices, request_assembly::BufferedBridgeRequest},
};

pub(super) fn request_model(request: &BufferedBridgeRequest) -> Option<String> {
    serde_json::from_slice::<serde_json::Value>(&request.body)
        .ok()?
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Refresh every candidate endpoint's quota snapshot before the unified
/// draw weights the pool, so all keys compete on the same snapshot.
pub(super) async fn refresh_candidate_quota(
    services: &RuntimeServices,
    candidate: &db::ModelRouteCandidate,
) {
    let Some(state) = services.admin_state() else {
        return;
    };
    for target in &candidate.targets {
        if let Err(error) = state
            .token_plan_quota
            .refresh_if_due(&state.pool, target.endpoint_id)
            .await
        {
            tracing::warn!(
                endpoint_id = %target.endpoint_id,
                error = %error,
                "quota refresh failed during unified key-pool selection"
            );
        }
    }
}
