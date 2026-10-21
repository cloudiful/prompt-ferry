use futures::future::BoxFuture;
use uuid::Uuid;

use crate::{
    db::ConfigRepository,
    endpoint_protocol::endpoint_protocol_client_for_endpoint,
    worker_admin::handlers::chatgpt_backend::{self, ChatgptBackendError, ChatgptQuota},
};

use super::ChatGptQuotaError;

pub(crate) trait ChatGptQuotaFetcher: Send + Sync {
    fn fetch<'a>(
        &'a self,
        endpoint_id: Uuid,
        repository: &'a ConfigRepository,
    ) -> BoxFuture<'a, Result<ChatgptQuota, ChatGptQuotaError>>;
}

pub(super) struct StoredOAuthQuotaFetcher;

impl ChatGptQuotaFetcher for StoredOAuthQuotaFetcher {
    fn fetch<'a>(
        &'a self,
        endpoint_id: Uuid,
        repository: &'a ConfigRepository,
    ) -> BoxFuture<'a, Result<ChatgptQuota, ChatGptQuotaError>> {
        Box::pin(async move {
            let endpoint = repository
                .get_endpoint(endpoint_id)
                .await
                .map_err(|_| ChatGptQuotaError::Storage)?
                .ok_or(ChatGptQuotaError::NotConfigured)?;
            if endpoint.provider != crate::db::EndpointProvider::OpenAi {
                return Err(ChatGptQuotaError::NotConfigured);
            }
            let proxy_url = repository
                .endpoint_proxy_url(endpoint_id)
                .await
                .map_err(|_| ChatGptQuotaError::Storage)?;
            let client = endpoint_protocol_client_for_endpoint(
                proxy_url.as_deref(),
                &chatgpt_backend::chatgpt_backend_base_url(),
            )
            .map_err(|_| ChatGptQuotaError::Upstream)?;
            let token = repository
                .get_endpoint_oauth_token(endpoint_id)
                .await
                .map_err(|_| ChatGptQuotaError::Storage)?
                .ok_or(ChatGptQuotaError::NotConfigured)?;
            // Issue #1004 P1: the admin credential probe and this quota
            // service share one resolution path, so an expired token is
            // refreshed (and the rotated credential persisted) identically.
            let (access_token, account_id) =
                match chatgpt_backend::resolve_subscription_access_token(
                    repository,
                    &client,
                    endpoint_id,
                    &token,
                )
                .await
                {
                    Ok(resolved) => resolved,
                    Err(ChatgptBackendError::InvalidGrant(_)) => {
                        return Err(ChatGptQuotaError::Auth);
                    }
                    Err(ChatgptBackendError::NotConfigured(_)) => {
                        return Err(ChatGptQuotaError::Storage);
                    }
                    Err(ChatgptBackendError::Timeout) => {
                        return Err(ChatGptQuotaError::Timeout);
                    }
                    Err(ChatgptBackendError::Upstream { .. }) => {
                        return Err(ChatGptQuotaError::Upstream);
                    }
                };
            chatgpt_backend::fetch_chatgpt_quota(&client, &access_token, account_id.as_deref())
                .await
                .map_err(|error| match error {
                    ChatgptBackendError::NotConfigured(_) => ChatGptQuotaError::NotConfigured,
                    ChatgptBackendError::InvalidGrant(_) => ChatGptQuotaError::Auth,
                    ChatgptBackendError::Timeout => ChatGptQuotaError::Timeout,
                    ChatgptBackendError::Upstream {
                        status: Some(200), ..
                    } => ChatGptQuotaError::InvalidQuota,
                    ChatgptBackendError::Upstream {
                        status: Some(401 | 403),
                        ..
                    } => ChatGptQuotaError::Auth,
                    ChatgptBackendError::Upstream { .. } => ChatGptQuotaError::Upstream,
                })
        })
    }
}
