use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    db::{self, ChatgptQuotaRefreshState, ChatgptQuotaSnapshot, quota_snapshots},
    worker_admin::chatgpt_quota_normalize,
    worker_admin_types::{SubscriptionQuotaObservation, TokenPlanUsageResponse},
};

use super::{
    ChatGptQuotaError, ChatGptQuotaService, RefreshCompletion,
    policy::{RefreshTrigger, stale},
};

impl ChatGptQuotaService {
    pub(crate) async fn read(
        &self,
        endpoint: &db::ProviderEndpoint,
        force: bool,
    ) -> Result<TokenPlanUsageResponse, ChatGptQuotaError> {
        let endpoint_id = endpoint.endpoint_id;
        let (snapshot, state) = self.load(endpoint_id).await?;
        if !force {
            if let Some(snapshot) = snapshot {
                return self.response(endpoint, snapshot, state, None);
            }
            if let Some(error) = state.as_ref().and_then(error_from_state)
                && state
                    .as_ref()
                    .and_then(|state| state.next_retry_at)
                    .is_some_and(|retry_at| retry_at > Utc::now())
            {
                return Err(error);
            }
        }

        let trigger = if force {
            RefreshTrigger::Manual
        } else {
            RefreshTrigger::Request
        };
        let completion = self
            .refresh_shared(endpoint_id, trigger, Utc::now(), None)
            .await;
        let Some(snapshot) = completion.snapshot else {
            return Err(completion.error.unwrap_or(ChatGptQuotaError::Upstream));
        };
        self.response(endpoint, snapshot, completion.state, completion.error)
    }

    pub(super) async fn load(
        &self,
        endpoint_id: Uuid,
    ) -> Result<
        (
            Option<ChatgptQuotaSnapshot>,
            Option<ChatgptQuotaRefreshState>,
        ),
        ChatGptQuotaError,
    > {
        let snapshot = quota_snapshots::latest_snapshot(&self.inner.pool, endpoint_id)
            .await
            .map_err(|_| ChatGptQuotaError::Storage)?;
        let state = quota_snapshots::get_refresh_state(&self.inner.pool, endpoint_id)
            .await
            .map_err(|_| ChatGptQuotaError::Storage)?;
        Ok((snapshot, state))
    }

    fn response(
        &self,
        endpoint: &db::ProviderEndpoint,
        snapshot: ChatgptQuotaSnapshot,
        state: Option<ChatgptQuotaRefreshState>,
        operation_error: Option<ChatGptQuotaError>,
    ) -> Result<TokenPlanUsageResponse, ChatGptQuotaError> {
        let now = Utc::now();
        let refreshing = state.as_ref().is_some_and(|state| {
            state.lease_owner.is_some()
                && state
                    .lease_expires_at
                    .is_some_and(|expires_at| expires_at > now)
        });
        let last_error_code = state
            .as_ref()
            .and_then(|state| state.last_error_code.clone())
            .or_else(|| operation_error.map(|error| error.code().to_string()));
        let observation = SubscriptionQuotaObservation {
            observed_at: Some(snapshot.observed_at),
            source: Some(snapshot.source.as_str().to_string()),
            stale: stale(snapshot.observed_at, now) || last_error_code.is_some(),
            last_error_code,
            next_retry_at: state.and_then(|state| state.next_retry_at),
            refreshing,
        };
        chatgpt_quota_normalize::response_from_snapshot(endpoint, &snapshot, observation)
            .map_err(|_| ChatGptQuotaError::InvalidQuota)
    }
}

pub(super) fn completed_after(
    trigger: RefreshTrigger,
    arrival: DateTime<Utc>,
    snapshot: Option<&ChatgptQuotaSnapshot>,
    state: Option<&ChatgptQuotaRefreshState>,
) -> bool {
    if trigger.is_manual() {
        return snapshot.is_some_and(|snapshot| snapshot.observed_at >= arrival)
            || state.is_some_and(|state| {
                state.lease_owner.is_none()
                    && state
                        .last_attempt_at
                        .is_some_and(|completed_at| completed_at >= arrival)
            });
    }
    snapshot.is_some_and(|snapshot| snapshot.observed_at >= arrival)
        || state.is_some_and(|state| {
            state.lease_owner.is_none()
                && state
                    .last_attempt_at
                    .is_some_and(|completed_at| completed_at >= arrival)
                && state.last_error_code.is_some()
        })
}

pub(super) fn completion(
    snapshot: Option<ChatgptQuotaSnapshot>,
    state: Option<ChatgptQuotaRefreshState>,
    fallback_error: Option<ChatGptQuotaError>,
) -> RefreshCompletion {
    let error = state.as_ref().and_then(error_from_state).or(fallback_error);
    RefreshCompletion {
        snapshot,
        state,
        error,
    }
}

pub(super) fn error_from_state(state: &ChatgptQuotaRefreshState) -> Option<ChatGptQuotaError> {
    match state.last_error_code.as_deref() {
        Some("not_configured") => Some(ChatGptQuotaError::NotConfigured),
        Some("auth") => Some(ChatGptQuotaError::Auth),
        Some("timeout") => Some(ChatGptQuotaError::Timeout),
        Some("invalid_quota") => Some(ChatGptQuotaError::InvalidQuota),
        Some("storage") => Some(ChatGptQuotaError::Storage),
        Some("in_progress") => Some(ChatGptQuotaError::InProgress),
        Some(_) => Some(ChatGptQuotaError::Upstream),
        None => None,
    }
}
