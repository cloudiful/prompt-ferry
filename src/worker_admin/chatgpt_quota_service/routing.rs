use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::{
    db::{EndpointPlan, EndpointProvider},
    worker_admin_types::SubscriptionWindowUsage,
};

use super::{ChatGptQuotaError, ChatGptQuotaService, policy::stale};

const ROUTING_READ_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub(crate) struct ChatGptQuotaRoutingSnapshot {
    pub observed_at: chrono::DateTime<Utc>,
    pub limit_reached: Option<bool>,
    pub windows: Vec<SubscriptionWindowUsage>,
    pub stale: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum RoutingSnapshot {
    NoSubscription,
    Subscription(Option<ChatGptQuotaRoutingSnapshot>),
}

impl ChatGptQuotaService {
    pub(crate) async fn routing_snapshot(
        &self,
        endpoint_id: Uuid,
    ) -> Result<RoutingSnapshot, ChatGptQuotaError> {
        match tokio::time::timeout(
            ROUTING_READ_TIMEOUT,
            self.routing_snapshot_local(endpoint_id),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(ChatGptQuotaError::Timeout),
        }
    }

    async fn routing_snapshot_local(
        &self,
        endpoint_id: Uuid,
    ) -> Result<RoutingSnapshot, ChatGptQuotaError> {
        let endpoint = self
            .inner
            .repository
            .get_endpoint(endpoint_id)
            .await
            .map_err(|_| ChatGptQuotaError::Storage)?;
        let Some(endpoint) = endpoint.filter(|endpoint| {
            endpoint.provider == EndpointProvider::OpenAi
                && endpoint.plan == EndpointPlan::ChatgptSubscription
        }) else {
            return Ok(RoutingSnapshot::NoSubscription);
        };
        let (snapshot, state) = self.load(endpoint.endpoint_id).await?;
        let Some(snapshot) = snapshot else {
            return Ok(RoutingSnapshot::Subscription(None));
        };
        let windows = serde_json::from_value(snapshot.windows)
            .map_err(|_| ChatGptQuotaError::InvalidQuota)?;
        let now = Utc::now();
        Ok(RoutingSnapshot::Subscription(Some(
            ChatGptQuotaRoutingSnapshot {
                observed_at: snapshot.observed_at,
                limit_reached: snapshot.limit_reached,
                windows,
                stale: stale(snapshot.observed_at, now)
                    || state
                        .as_ref()
                        .and_then(|state| state.last_error_code.as_ref())
                        .is_some(),
            },
        )))
    }
}
