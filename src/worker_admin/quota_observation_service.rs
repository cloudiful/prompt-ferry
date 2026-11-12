//! Shared persistence for new quota fetch results (issue #863 P2).
//!
//! Every actual provider fetch (manual, request-driven, or periodic sampling)
//! lands here with the timestamp captured at fetch completion. Cache hits and
//! singleflight reuse never reach this module, so no extra row is written for
//! a reused observation. Writes are best-effort: a storage failure is logged
//! with a constant message plus the endpoint id (never a secret or an error
//! body) and never discards the fresh in-memory usage or breaks routing.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::{
    EndpointProvider, MAX_ERROR_CODE_CHARS, MAX_KEY_LABEL_CHARS, ProviderEndpoint,
    QuotaObservationCreate, QuotaObservationProvider, QuotaObservationSource,
    quota_key_observations,
};
use crate::worker_admin_types::TokenPlanUsageResponse;

/// Map a fetch provider to its storage identity. `Generic` has no normalizer
/// and never persists; every other provider (including `OpenAi`, which the
/// ChatGPT service mirrors separately) maps one-to-one.
pub(crate) fn observation_provider(provider: EndpointProvider) -> Option<QuotaObservationProvider> {
    match provider {
        EndpointProvider::Generic => None,
        EndpointProvider::Minimax => Some(QuotaObservationProvider::Minimax),
        EndpointProvider::CommandCode => Some(QuotaObservationProvider::CommandCode),
        EndpointProvider::OpencodeGo => Some(QuotaObservationProvider::OpencodeGo),
        EndpointProvider::OpenRouter => Some(QuotaObservationProvider::Openrouter),
        EndpointProvider::Glm => Some(QuotaObservationProvider::Glm),
        EndpointProvider::DeepSeek => Some(QuotaObservationProvider::Deepseek),
        EndpointProvider::OpenAi => Some(QuotaObservationProvider::Openai),
    }
}

/// Keep a stable provider code, or fall back to an honest `unknown` when the
/// fetch reports nothing usable. The fallback never carries provider text, so
/// no secret or raw response can leak through this path.
pub(crate) fn sanitize_error_code(code: Option<&str>) -> Option<String> {
    let code = code?;
    if !code.is_empty()
        && code.chars().count() <= MAX_ERROR_CODE_CHARS
        && code.chars().all(|char| {
            char.is_ascii_lowercase() || char.is_ascii_digit() || matches!(char, '_' | ':' | '-')
        })
    {
        Some(code.to_string())
    } else {
        Some("unknown".to_string())
    }
}

/// Key labels are metadata, never secrets, but storage bounds them at 200
/// characters; truncate on a character boundary instead of dropping the row.
pub(crate) fn truncate_label(label: &str) -> String {
    if label.chars().count() <= MAX_KEY_LABEL_CHARS {
        label.to_string()
    } else {
        label.chars().take(MAX_KEY_LABEL_CHARS).collect()
    }
}

/// Enabled key identities from an already-loaded endpoint configuration.
/// Secrets stay out: only the key id and label leave this helper.
pub(crate) fn enabled_key_identities(endpoint: &ProviderEndpoint) -> Vec<(Uuid, String)> {
    let keys = endpoint
        .api_keys
        .iter()
        .filter(|key| key.enabled && !key.api_key.trim().is_empty())
        .map(|key| (key.key_id, key.key_label.clone()))
        .collect::<Vec<_>>();
    if keys.is_empty() && !endpoint.api_key.trim().is_empty() {
        vec![(Uuid::nil(), endpoint.name.clone())]
    } else {
        keys
    }
}

/// Persist one fetch result, key by key. Successful keys carry their typed
/// canonical windows (`None` reads as an explicit empty); failed keys carry
/// `ok=false` with empty windows and a sanitized code. Generic providers
/// without plan metadata persist null plan fields. Every write is
/// best-effort: failures log a constant message with the endpoint id and the
/// call still succeeds so the cache keeps serving the fresh usage.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn persist_response(
    pool: &PgPool,
    endpoint_id: Uuid,
    provider: EndpointProvider,
    response: &TokenPlanUsageResponse,
    source: QuotaObservationSource,
    observed_at: DateTime<Utc>,
    plan_type: Option<String>,
    limit_reached: Option<bool>,
) {
    let Some(storage_provider) = observation_provider(provider) else {
        return;
    };
    for key in &response.keys {
        let windows = if key.ok {
            key.quota_windows.clone().unwrap_or_default()
        } else {
            Vec::new()
        };
        let create = match QuotaObservationCreate::new(
            endpoint_id,
            key.key_id,
            storage_provider,
            truncate_label(&key.key_label),
            source,
            observed_at,
            key.ok,
            key.status.map(|status| status.to_string()),
            sanitize_error_code(key.error_code.as_deref()),
            plan_type.clone(),
            limit_reached,
            windows,
        ) {
            Ok(create) => create,
            Err(_) => {
                tracing::warn!(
                    endpoint_id = %endpoint_id,
                    "quota observation persist skipped an invalid observation"
                );
                continue;
            }
        };
        if quota_key_observations::insert_observation(pool, &create)
            .await
            .is_err()
        {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                "quota observation persist failed"
            );
        }
    }
}

/// Persist an endpoint-level fetch failure for every enabled key from the
/// already-loaded endpoint configuration. The rows are honestly unknown:
/// `ok=false`, empty windows, no status, and a constant `upstream` code that
/// carries no provider text. Best-effort like [`persist_response`].
pub(crate) async fn persist_endpoint_failure(
    pool: &PgPool,
    endpoint_id: Uuid,
    provider: EndpointProvider,
    keys: &[(Uuid, String)],
    source: QuotaObservationSource,
    observed_at: DateTime<Utc>,
) {
    let Some(storage_provider) = observation_provider(provider) else {
        return;
    };
    for (key_id, key_label) in keys {
        let create = match QuotaObservationCreate::new(
            endpoint_id,
            *key_id,
            storage_provider,
            truncate_label(key_label),
            source,
            observed_at,
            false,
            None,
            Some("upstream".to_string()),
            None,
            None,
            Vec::new(),
        ) {
            Ok(create) => create,
            Err(_) => continue,
        };
        if quota_key_observations::insert_observation(pool, &create)
            .await
            .is_err()
        {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                "quota observation persist failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_mapping_excludes_generic_only() {
        assert!(observation_provider(EndpointProvider::Generic).is_none());
        assert_eq!(
            observation_provider(EndpointProvider::Minimax),
            Some(QuotaObservationProvider::Minimax)
        );
        assert_eq!(
            observation_provider(EndpointProvider::OpenAi),
            Some(QuotaObservationProvider::Openai)
        );
    }

    #[test]
    fn error_codes_stay_stable_and_never_carry_provider_text() {
        assert_eq!(sanitize_error_code(None), None);
        assert_eq!(
            sanitize_error_code(Some("upstream")),
            Some("upstream".to_string())
        );
        assert_eq!(
            sanitize_error_code(Some("BAD CODE!")),
            Some("unknown".to_string())
        );
        assert_eq!(sanitize_error_code(Some("")), Some("unknown".to_string()));
        assert_eq!(
            sanitize_error_code(Some("x".repeat(65).as_str())),
            Some("unknown".to_string())
        );
    }

    #[test]
    fn labels_truncate_on_char_boundaries() {
        assert_eq!(truncate_label("k"), "k");
        assert_eq!(truncate_label(&"k".repeat(201)).chars().count(), 200);
    }
}
