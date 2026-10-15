use super::{
    delete_expired_observations, eligible_endpoints, insert_observation, latest_observation,
    list_observations, observation_endpoint,
};
use crate::db::types::{QuotaObservationCreate, QuotaObservationProvider, QuotaObservationSource};
use crate::worker_admin_types::{
    CanonicalQuotaWindow, CanonicalQuotaWindowKind, SubscriptionWindowAvailability,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use test_support::{cleanup_fixtures, create_endpoint, create_key, test_pool};

fn observed_at(hour: u32) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-10-10T{hour:02}:00:00Z"))
        .expect("fixed test timestamp parses")
        .with_timezone(&Utc)
}

fn known_window(source_id: &str, remaining_percent: f64) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Rolling,
        source_model: Some("test-model".to_string()),
        unit: Some("tokens".to_string()),
        duration_seconds: Some(300),
        is_default_scope: true,
        availability: SubscriptionWindowAvailability::Known,
        remaining_percent: Some(remaining_percent),
        current: Some(100.0 - remaining_percent),
        limit: Some(100.0),
        remaining: None,
        reset_at: None,
    }
}

fn unknown_window(source_id: &str) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Rolling,
        source_model: None,
        unit: None,
        duration_seconds: None,
        is_default_scope: true,
        availability: SubscriptionWindowAvailability::Unknown,
        remaining_percent: None,
        current: None,
        limit: None,
        remaining: None,
        reset_at: None,
    }
}

fn balance_window(source_id: &str, remaining: f64) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Balance,
        source_model: None,
        unit: Some("credits".to_string()),
        duration_seconds: None,
        is_default_scope: false,
        availability: SubscriptionWindowAvailability::Known,
        remaining_percent: None,
        current: None,
        limit: None,
        remaining: Some(remaining),
        reset_at: None,
    }
}

fn success_create(
    endpoint_id: Uuid,
    key_id: Uuid,
    observed_at: DateTime<Utc>,
) -> QuotaObservationCreate {
    QuotaObservationCreate::new(
        endpoint_id,
        key_id,
        QuotaObservationProvider::Minimax,
        "test-key".to_string(),
        QuotaObservationSource::Request,
        observed_at,
        true,
        None,
        None,
        Some("team".to_string()),
        Some(false),
        vec![
            known_window("usage", 75.0),
            known_window("empty-slot", 0.0),
            unknown_window("unsupported-slot"),
            balance_window("credits", 50.0),
        ],
    )
    .expect("valid success observation builds")
}

#[tokio::test]
async fn insert_and_latest_roundtrip_preserves_presence() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true).await;
    let key_id = create_key(&pool, endpoint_id, "test-key", true).await;
    let create = success_create(endpoint_id, key_id, observed_at(10));

    let stored = insert_observation(&pool, &create).await.unwrap();
    assert_eq!(stored.endpoint_id, endpoint_id);
    assert_eq!(stored.key_id, key_id);
    assert_eq!(stored.provider, QuotaObservationProvider::Minimax);
    assert_eq!(stored.key_label, "test-key");
    assert_eq!(stored.source, QuotaObservationSource::Request);
    assert_eq!(stored.observed_at, observed_at(10));
    assert!(stored.ok);
    assert_eq!(stored.plan_type.as_deref(), Some("team"));
    assert_eq!(stored.limit_reached, Some(false));
    assert_eq!(stored.windows, create.windows);
    // A real zero stays a real zero and does not collapse into unknown.
    assert_eq!(stored.windows[1].remaining_percent, Some(0.0));
    assert_eq!(stored.windows[2].remaining_percent, None);

    let latest = latest_observation(&pool, endpoint_id, key_id)
        .await
        .unwrap()
        .expect("inserted observation is latest");
    assert_eq!(latest.observation_id, stored.observation_id);
    cleanup_fixtures(&pool, endpoint_id).await;
}

#[tokio::test]
async fn duplicate_observation_is_rejected_not_duplicated() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true).await;
    let key_id = create_key(&pool, endpoint_id, "test-key", true).await;
    let create = success_create(endpoint_id, key_id, observed_at(10));

    insert_observation(&pool, &create).await.unwrap();
    assert!(
        insert_observation(&pool, &create).await.is_err(),
        "the same endpoint/key/observed_at must not create a second row"
    );
    assert_eq!(
        list_observations(&pool, endpoint_id, key_id, None, 200)
            .await
            .unwrap()
            .len(),
        1
    );
    cleanup_fixtures(&pool, endpoint_id).await;
}

#[tokio::test]
async fn failure_observation_roundtrip() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true).await;
    let key_id = create_key(&pool, endpoint_id, "test-key", true).await;
    let create = QuotaObservationCreate::new(
        endpoint_id,
        key_id,
        QuotaObservationProvider::Openai,
        "test-key".to_string(),
        QuotaObservationSource::Periodic,
        observed_at(11),
        false,
        Some("error".to_string()),
        Some("upstream".to_string()),
        None,
        None,
        vec![],
    )
    .expect("valid failure observation builds");

    let stored = insert_observation(&pool, &create).await.unwrap();
    assert!(!stored.ok);
    assert_eq!(stored.status.as_deref(), Some("error"));
    assert_eq!(stored.error_code.as_deref(), Some("upstream"));
    assert!(stored.windows.is_empty());
    cleanup_fixtures(&pool, endpoint_id).await;
}

#[tokio::test]
async fn constructor_rejects_invalid_observations() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let valid_window = known_window("usage", 75.0);
    let build = |windows: Vec<CanonicalQuotaWindow>| {
        QuotaObservationCreate::new(
            endpoint_id,
            key_id,
            QuotaObservationProvider::Minimax,
            "test-key".to_string(),
            QuotaObservationSource::Request,
            observed_at(10),
            true,
            None,
            None,
            None,
            None,
            windows,
        )
    };

    let mut non_finite = valid_window.clone();
    non_finite.remaining_percent = Some(f64::INFINITY);
    let mut over_percent = valid_window.clone();
    over_percent.remaining_percent = Some(100.5);
    let mut synthetic_balance = balance_window("credits", 50.0);
    synthetic_balance.remaining_percent = Some(10.0);
    let mut synthetic_unknown = unknown_window("slot");
    synthetic_unknown.remaining_percent = Some(0.0);
    let mut empty_source = valid_window.clone();
    empty_source.source_id = String::new();
    let mut long_source = valid_window.clone();
    long_source.source_id = "s".repeat(129);
    let mut zero_duration = valid_window.clone();
    zero_duration.duration_seconds = Some(0);

    for (label, windows) in [
        ("non-finite percent", vec![non_finite]),
        ("percent over 100", vec![over_percent]),
        ("balance with percent", vec![synthetic_balance]),
        ("unknown with percent", vec![synthetic_unknown]),
        ("empty source id", vec![empty_source]),
        ("129-char source id", vec![long_source]),
        ("zero duration", vec![zero_duration]),
        (
            "65 windows",
            (0..65)
                .map(|index| known_window(&format!("slot-{index}"), 50.0))
                .collect(),
        ),
    ] {
        assert!(
            build(windows).is_err(),
            "{label} must be rejected without a database"
        );
    }

    let mut failed_with_windows = success_create(endpoint_id, key_id, observed_at(10));
    failed_with_windows.ok = false;
    assert!(
        failed_with_windows.validate().is_err(),
        "a failed observation carrying windows must be rejected"
    );
    assert!(
        QuotaObservationCreate::new(
            endpoint_id,
            key_id,
            QuotaObservationProvider::Minimax,
            "k".repeat(201),
            QuotaObservationSource::Request,
            observed_at(10),
            true,
            None,
            None,
            None,
            None,
            vec![valid_window.clone()],
        )
        .is_err(),
        "a 201-char key label must be rejected"
    );
    assert!(
        QuotaObservationCreate::new(
            endpoint_id,
            key_id,
            QuotaObservationProvider::Minimax,
            "test-key".to_string(),
            QuotaObservationSource::Request,
            observed_at(10),
            false,
            None,
            Some("BAD CODE!".to_string()),
            None,
            None,
            vec![],
        )
        .is_err(),
        "a non-stable error code must be rejected"
    );
}

#[tokio::test]
async fn list_observations_pages_by_cursor_and_clamps_limit() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true).await;
    let key_id = create_key(&pool, endpoint_id, "test-key", true).await;
    for hour in [8, 9, 10] {
        insert_observation(
            &pool,
            &success_create(endpoint_id, key_id, observed_at(hour)),
        )
        .await
        .unwrap();
    }

    let page = list_observations(&pool, endpoint_id, key_id, None, 2)
        .await
        .unwrap();
    assert_eq!(page.len(), 2);
    assert!(page[0].observed_at > page[1].observed_at);
    let rest = list_observations(&pool, endpoint_id, key_id, Some(page[1].observation_id), 2)
        .await
        .unwrap();
    assert_eq!(rest.len(), 1);
    assert!(rest[0].observed_at < page[1].observed_at);
    // Limits clamp to 1..=200 instead of erroring or scanning unbounded.
    assert_eq!(
        list_observations(&pool, endpoint_id, key_id, None, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        list_observations(&pool, endpoint_id, key_id, None, 10_000)
            .await
            .unwrap()
            .len(),
        3
    );
    cleanup_fixtures(&pool, endpoint_id).await;
}

#[tokio::test]
async fn latest_and_endpoint_pair_are_key_scoped() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true).await;
    let first_key = create_key(&pool, endpoint_id, "first-key", true).await;
    let second_key = create_key(&pool, endpoint_id, "second-key", true).await;
    insert_observation(
        &pool,
        &success_create(endpoint_id, first_key, observed_at(9)),
    )
    .await
    .unwrap();
    insert_observation(
        &pool,
        &success_create(endpoint_id, second_key, observed_at(10)),
    )
    .await
    .unwrap();

    let latest_first = latest_observation(&pool, endpoint_id, first_key)
        .await
        .unwrap()
        .expect("first key has an observation");
    assert_eq!(latest_first.observed_at, observed_at(9));
    let pair = observation_endpoint(&pool, endpoint_id, second_key)
        .await
        .unwrap()
        .expect("configured pair resolves");
    assert_eq!(pair.provider, QuotaObservationProvider::Openai);
    assert!(
        observation_endpoint(&pool, endpoint_id, Uuid::new_v4())
            .await
            .unwrap()
            .is_none()
    );
    cleanup_fixtures(&pool, endpoint_id).await;
}

#[tokio::test]
async fn eligible_endpoints_cover_only_enabled_supported_pairs() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let enabled_endpoint = create_endpoint(&pool, "minimax", true).await;
    let enabled_key = create_key(&pool, enabled_endpoint, "live-key", true).await;
    let disabled_key = create_key(&pool, enabled_endpoint, "off-key", false).await;
    let disabled_endpoint = create_endpoint(&pool, "minimax", false).await;
    let _disabled_key = create_key(&pool, disabled_endpoint, "live-key", true).await;
    let generic_endpoint = create_endpoint(&pool, "generic", true).await;
    let _generic_key = create_key(&pool, generic_endpoint, "live-key", true).await;

    let eligible = eligible_endpoints(&pool).await.unwrap();
    assert!(
        eligible
            .iter()
            .any(|row| row.endpoint_id == enabled_endpoint && row.key_id == enabled_key),
        "an enabled pair must be eligible"
    );
    assert!(
        !eligible.iter().any(|row| row.key_id == disabled_key),
        "a disabled key must not be eligible"
    );
    assert!(
        !eligible
            .iter()
            .any(|row| row.endpoint_id == disabled_endpoint),
        "a disabled endpoint must not be eligible"
    );
    assert!(
        !eligible
            .iter()
            .any(|row| row.endpoint_id == generic_endpoint),
        "a generic endpoint has no normalizer and must not be eligible"
    );
    cleanup_fixtures(&pool, enabled_endpoint).await;
    cleanup_fixtures(&pool, disabled_endpoint).await;
    cleanup_fixtures(&pool, generic_endpoint).await;
}

#[tokio::test]
async fn expired_observations_prune_in_bounded_batches() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true).await;
    let key_id = create_key(&pool, endpoint_id, "test-key", true).await;
    let old = DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
        .expect("fixed test timestamp parses")
        .with_timezone(&Utc);
    insert_observation(&pool, &success_create(endpoint_id, key_id, old))
        .await
        .unwrap();
    insert_observation(&pool, &success_create(endpoint_id, key_id, observed_at(10)))
        .await
        .unwrap();

    assert_eq!(
        delete_expired_observations(&pool, Utc::now(), 1_000)
            .await
            .unwrap(),
        1
    );
    let latest = latest_observation(&pool, endpoint_id, key_id)
        .await
        .unwrap()
        .expect("recent observation survives pruning");
    assert_eq!(latest.observed_at, observed_at(10));
    cleanup_fixtures(&pool, endpoint_id).await;
}

#[tokio::test]
async fn only_owned_fixtures_are_removed() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true).await;
    let key_id = create_key(&pool, endpoint_id, "test-key", true).await;
    insert_observation(&pool, &success_create(endpoint_id, key_id, observed_at(10)))
        .await
        .unwrap();
    cleanup_fixtures(&pool, endpoint_id).await;

    assert!(
        latest_observation(&pool, endpoint_id, key_id)
            .await
            .unwrap()
            .is_none()
    );
    for orphan in [
        "8c6e4e55-967c-4267-a45a-24647d2afad0",
        "898b022d-51be-418e-a5e9-a78c5de92ebf",
        "eedecbee-df89-40fc-9eb7-e147cf55f373",
        "ccce65ec-671c-4db0-8ddf-5afa11372499",
    ] {
        let orphan_id = Uuid::parse_str(orphan).expect("known orphan UUID parses");
        let name: Option<String> =
            sqlx::query_scalar("SELECT name FROM provider_endpoints WHERE endpoint_id = $1")
                .bind(orphan_id)
                .fetch_optional(&pool)
                .await
                .expect("historical orphans stay readable");
        assert_eq!(
            name.as_deref(),
            Some(format!("quota-service-{orphan}").as_str()),
            "historical orphan {orphan} must keep its exact name"
        );
    }
}

pub(super) mod test_support {
    use sqlx::PgPool;
    use uuid::Uuid;

    #[allow(dead_code)]
    mod test_pg_target {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/test_pg_target.rs"
        ));
    }

    // Only an absent pair of dedicated keys skips. A configured target that is
    // malformed or disagrees with its confirmed identity fails here rather
    // than degrading into a silent skip.
    pub(in crate::db::quota_key_observations) async fn test_pool() -> Option<PgPool> {
        match test_pg_target::optional_pool(4).await {
            Ok(Some(pool)) => Some(pool),
            Ok(None) => {
                eprintln!(
                    "skipped quota observation database setup: neither {} nor {} is configured",
                    test_pg_target::DATABASE_URL_KEY,
                    test_pg_target::CONFIRMED_TARGET_KEY
                );
                None
            }
            Err(error) => panic!("quota observation test target is not usable: {error}"),
        }
    }

    pub(in crate::db::quota_key_observations) async fn create_endpoint(
        pool: &PgPool,
        provider: &str,
        enabled: bool,
    ) -> Uuid {
        let endpoint_id = Uuid::new_v4();
        // Minimax endpoints require a region by database constraint; every
        // other test provider takes NULL.
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
                     'responses', 'manual', 'quota-observation-fixture-key', $5)",
        )
        .bind(endpoint_id)
        .bind(format!("quota-observation-{endpoint_id}"))
        .bind(provider)
        .bind(region)
        .bind(enabled)
        .execute(pool)
        .await
        .expect("insert isolated observation endpoint");
        endpoint_id
    }

    pub(in crate::db::quota_key_observations) async fn create_key(
        pool: &PgPool,
        endpoint_id: Uuid,
        key_label: &str,
        enabled: bool,
    ) -> Uuid {
        let key_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO endpoint_api_keys (key_id, endpoint_id, key_label, api_key, position, enabled) \
             VALUES ($1, $2, $3, 'quota-observation-fixture-secret', \
                     (SELECT COUNT(*) FROM endpoint_api_keys WHERE endpoint_id = $2), $4)",
        )
        .bind(key_id)
        .bind(endpoint_id)
        .bind(key_label)
        .bind(enabled)
        .execute(pool)
        .await
        .expect("insert isolated observation key");
        key_id
    }

    pub(in crate::db::quota_key_observations) async fn cleanup_fixtures(
        pool: &PgPool,
        endpoint_id: Uuid,
    ) {
        sqlx::query("DELETE FROM provider_endpoints WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .execute(pool)
            .await
            .expect("remove isolated observation fixtures");
    }
}
