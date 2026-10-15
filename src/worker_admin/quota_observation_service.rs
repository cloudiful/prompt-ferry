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
        if let Err(_) = quota_key_observations::insert_observation(pool, &create).await {
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
        if let Err(_) = quota_key_observations::insert_observation(pool, &create).await {
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

    mod db_tests {
        use super::*;
        use crate::worker_admin_types::{
            CanonicalQuotaWindow, CanonicalQuotaWindowKind, SubscriptionWindowAvailability,
        };

        #[allow(dead_code)]
        mod test_pg_target {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/test_pg_target.rs"
            ));
        }

        async fn test_pool() -> Option<PgPool> {
            match test_pg_target::optional_pool(4).await {
                Ok(pool) => pool,
                Err(error) => {
                    panic!("quota observation service test target is not usable: {error}")
                }
            }
        }

        async fn create_endpoint(pool: &PgPool, provider: &str) -> Uuid {
            let endpoint_id = Uuid::new_v4();
            let region = if provider == "minimax" {
                Some("global")
            } else {
                None
            };
            sqlx::query(
                "INSERT INTO provider_endpoints \
                 (endpoint_id, scope, owner_user_id, name, provider, provider_region, base_url, \
                  native_api, native_api_source, api_key, enabled) \
                 VALUES ($1, 'admin', NULL, $2, $3, $4, 'https://example.invalid/v1', \
                         'responses', 'manual', 'service-fixture-key', true)",
            )
            .bind(endpoint_id)
            .bind(format!("quota-service-{endpoint_id}"))
            .bind(provider)
            .bind(region)
            .execute(pool)
            .await
            .expect("insert isolated service endpoint");
            endpoint_id
        }

        async fn create_key(pool: &PgPool, endpoint_id: Uuid, key_label: &str) -> Uuid {
            let key_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO endpoint_api_keys (key_id, endpoint_id, key_label, api_key, position, enabled) \
                 VALUES ($1, $2, $3, 'service-fixture-secret', \
                         (SELECT COUNT(*) FROM endpoint_api_keys WHERE endpoint_id = $2), true)",
            )
            .bind(key_id)
            .bind(endpoint_id)
            .bind(key_label)
            .execute(pool)
            .await
            .expect("insert isolated service key");
            key_id
        }

        async fn cleanup(pool: &PgPool, endpoint_id: Uuid) {
            sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
                .bind(endpoint_id)
                .execute(pool)
                .await
                .expect("remove isolated service fixtures");
        }

        fn known_window() -> CanonicalQuotaWindow {
            CanonicalQuotaWindow {
                source_id: "opencode_go:rolling".to_string(),
                kind: CanonicalQuotaWindowKind::Rolling,
                source_model: None,
                unit: None,
                duration_seconds: None,
                is_default_scope: true,
                availability: SubscriptionWindowAvailability::Known,
                remaining_percent: Some(80.0),
                current: None,
                limit: None,
                remaining: None,
                reset_at: None,
            }
        }

        fn success_response(key_id: Uuid) -> TokenPlanUsageResponse {
            TokenPlanUsageResponse {
                provider: EndpointProvider::OpencodeGo,
                provider_region: None,
                local_today_tokens: None,
                keys: vec![crate::worker_admin_types::TokenPlanKeyUsage {
                    key_id,
                    key_label: "live-key".to_string(),
                    ok: true,
                    status: Some(200),
                    error_code: None,
                    error_message: None,
                    quota_status: None,
                    model_remains: Vec::new(),
                    balances: None,
                    five_hour: None,
                    weekly: None,
                    opencodego_rolling: None,
                    opencodego_weekly: None,
                    opencodego_monthly: None,
                    openrouter_balance: None,
                    openrouter_spend: None,
                    glm_five_hour: None,
                    glm_weekly: None,
                    deepseek_balance: None,
                    quota_windows: Some(vec![known_window()]),
                }],
            }
        }

        #[tokio::test]
        async fn success_and_failure_rows_roundtrip_with_null_plan_metadata() {
            let Some(pool) = test_pool().await else {
                return;
            };
            let endpoint_id = create_endpoint(&pool, "opencode_go").await;
            let key_id = create_key(&pool, endpoint_id, "live-key").await;
            let observed_at = Utc::now();

            persist_response(
                &pool,
                endpoint_id,
                EndpointProvider::OpencodeGo,
                &success_response(key_id),
                QuotaObservationSource::Request,
                observed_at,
                None,
                None,
            )
            .await;
            let latest = quota_key_observations::latest_observation(&pool, endpoint_id, key_id)
                .await
                .unwrap()
                .expect("success row persists");
            assert!(latest.ok);
            assert_eq!(latest.windows.len(), 1);
            assert!(latest.plan_type.is_none());
            assert!(latest.limit_reached.is_none());

            persist_endpoint_failure(
                &pool,
                endpoint_id,
                EndpointProvider::OpencodeGo,
                &[(key_id, "live-key".to_string())],
                QuotaObservationSource::Periodic,
                observed_at + chrono::Duration::seconds(1),
            )
            .await;
            let latest = quota_key_observations::latest_observation(&pool, endpoint_id, key_id)
                .await
                .unwrap()
                .expect("failure row persists");
            assert!(!latest.ok);
            assert!(latest.windows.is_empty());
            assert_eq!(latest.error_code.as_deref(), Some("upstream"));
            cleanup(&pool, endpoint_id).await;
        }

        #[tokio::test]
        async fn duplicate_observations_do_not_duplicate_rows() {
            let Some(pool) = test_pool().await else {
                return;
            };
            let endpoint_id = create_endpoint(&pool, "opencode_go").await;
            let key_id = create_key(&pool, endpoint_id, "live-key").await;
            let observed_at = Utc::now();
            let response = success_response(key_id);

            for _ in 0..2 {
                persist_response(
                    &pool,
                    endpoint_id,
                    EndpointProvider::OpencodeGo,
                    &response,
                    QuotaObservationSource::Request,
                    observed_at,
                    None,
                    None,
                )
                .await;
            }
            assert_eq!(
                quota_key_observations::list_observations(&pool, endpoint_id, key_id, None, 200)
                    .await
                    .unwrap()
                    .len(),
                1
            );
            cleanup(&pool, endpoint_id).await;
        }

        #[tokio::test]
        async fn invalid_observations_are_skipped_without_a_row() {
            let Some(pool) = test_pool().await else {
                return;
            };
            let endpoint_id = create_endpoint(&pool, "opencode_go").await;
            let key_id = create_key(&pool, endpoint_id, "live-key").await;
            let mut response = success_response(key_id);
            response.keys[0].quota_windows = Some(vec![CanonicalQuotaWindow {
                remaining_percent: Some(f64::NAN),
                ..known_window()
            }]);

            persist_response(
                &pool,
                endpoint_id,
                EndpointProvider::OpencodeGo,
                &response,
                QuotaObservationSource::Request,
                Utc::now(),
                None,
                None,
            )
            .await;
            assert!(
                quota_key_observations::latest_observation(&pool, endpoint_id, key_id)
                    .await
                    .unwrap()
                    .is_none()
            );
            cleanup(&pool, endpoint_id).await;
        }

        #[tokio::test]
        async fn generic_responses_never_persist() {
            let Some(pool) = test_pool().await else {
                return;
            };
            let endpoint_id = create_endpoint(&pool, "generic").await;
            let key_id = create_key(&pool, endpoint_id, "live-key").await;
            let mut response = success_response(key_id);
            response.provider = EndpointProvider::Generic;

            persist_response(
                &pool,
                endpoint_id,
                EndpointProvider::Generic,
                &response,
                QuotaObservationSource::Request,
                Utc::now(),
                None,
                None,
            )
            .await;
            assert!(
                quota_key_observations::latest_observation(&pool, endpoint_id, key_id)
                    .await
                    .unwrap()
                    .is_none()
            );
            cleanup(&pool, endpoint_id).await;
        }
    }
}
