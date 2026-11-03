use super::*;

#[test]
fn clones_isolate_keys_and_normalize_synthetic_identity() {
    let store = QuotaFeedbackStore::default();
    let clone = store.clone();
    let now = Instant::now();
    let endpoint = Uuid::new_v4();
    let a = QuotaFeedbackScope::new(endpoint, Some(Uuid::new_v4()));
    let b = QuotaFeedbackScope::new(endpoint, Some(Uuid::new_v4()));
    let synthetic = QuotaFeedbackScope::new(endpoint, Some(Uuid::nil()));
    assert_eq!(synthetic, QuotaFeedbackScope::new(endpoint, None));
    store.record_429(a, None, now, now);
    assert_eq!(clone.eligibility(a, now), QuotaFeedbackEligibility::Cooling);
    assert_eq!(
        clone.eligibility(b, now),
        QuotaFeedbackEligibility::Eligible
    );
    assert_eq!(
        clone.eligibility(synthetic, now),
        QuotaFeedbackEligibility::Eligible
    );
}

#[test]
fn cooldown_backoff_probe_ownership_and_newer_failure() {
    let store = QuotaFeedbackStore::default();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
    let start = Instant::now();
    store.record_429(scope, None, start, start);
    let probe = start + Duration::from_secs(30);
    assert_eq!(
        store.eligibility(scope, probe),
        QuotaFeedbackEligibility::ProbeRequired
    );
    assert!(store.reserve_probe(scope, probe));
    assert!(!store.reserve_probe(scope, probe));
    store.release_probe(scope, start, probe);
    assert_eq!(
        store.eligibility(scope, probe),
        QuotaFeedbackEligibility::ProbeBusy
    );
    let next = probe + PROBE_TTL;
    assert!(store.reserve_probe(scope, next));
    store.record_429(scope, None, next, next);
    store.record_success(scope, probe, next);
    assert_eq!(
        store.eligibility(scope, next + Duration::from_secs(59)),
        QuotaFeedbackEligibility::Cooling
    );
    assert_eq!(
        store.eligibility(scope, next + Duration::from_secs(60)),
        QuotaFeedbackEligibility::ProbeRequired
    );
    for _ in 0..20 {
        store.record_429(scope, None, next, next);
    }
    assert_eq!(
        store.eligibility(scope, next + Duration::from_secs(299)),
        QuotaFeedbackEligibility::Cooling
    );
    assert_eq!(
        store.eligibility(scope, next + Duration::from_secs(300)),
        QuotaFeedbackEligibility::ProbeRequired
    );
    store.record_success(scope, next, next);
    assert_eq!(
        store.eligibility(scope, next),
        QuotaFeedbackEligibility::Eligible
    );
}

#[test]
fn every_cause_shares_the_backoff_and_keeps_its_own_label() {
    let store = QuotaFeedbackStore::default();
    let now = Instant::now();
    for (cause, expected) in [
        (
            QuotaFeedbackCause::RateLimited,
            QuotaFeedbackCause::RateLimited,
        ),
        (
            QuotaFeedbackCause::UpstreamUnavailable,
            QuotaFeedbackCause::UpstreamUnavailable,
        ),
        (QuotaFeedbackCause::Exhausted, QuotaFeedbackCause::Exhausted),
    ] {
        let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
        store.record_failure(scope, cause, Some(Duration::from_secs(120)), now, now);
        // Identical deadline and identical routing effect for every cause.
        assert_eq!(
            store.eligibility(scope, now),
            QuotaFeedbackEligibility::Cooling
        );
        let (recorded, remaining) = store
            .active_cooldown(scope, now)
            .expect("active block keeps its cause");
        assert_eq!(recorded, expected);
        assert_eq!(remaining, Duration::from_secs(120));
    }
}

#[test]
fn the_default_429_caller_records_a_rate_limit_not_an_exhaustion() {
    let store = QuotaFeedbackStore::default();
    let now = Instant::now();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
    store.record_429(scope, None, now, now);
    assert_eq!(
        store.active_cooldown(scope, now).map(|(cause, _)| cause),
        Some(QuotaFeedbackCause::RateLimited)
    );
    // A newer failure replaces the cause instead of keeping the first one.
    let later = now + Duration::from_secs(10);
    store.record_failure(scope, QuotaFeedbackCause::Exhausted, None, later, later);
    assert_eq!(
        store.active_cooldown(scope, later).map(|(cause, _)| cause),
        Some(QuotaFeedbackCause::Exhausted)
    );
}

#[test]
fn retry_after_cap_and_bounded_retention() {
    let store = QuotaFeedbackStore::default();
    let now = Instant::now();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
    store.record_429(scope, Some(Duration::from_secs(10000)), now, now);
    assert_eq!(
        store.eligibility(scope, now + Duration::from_secs(3600)),
        QuotaFeedbackEligibility::ProbeRequired
    );
    assert_eq!(
        store.eligibility(scope, now + RETENTION),
        QuotaFeedbackEligibility::Eligible
    );
    store.record_429(scope, None, now, now);
    for _ in 0..4106 {
        store.record_429(
            QuotaFeedbackScope::new(Uuid::new_v4(), None),
            None,
            now,
            now,
        );
    }
    assert_eq!(store.0.lock().unwrap().feedback.len(), 4107);
    assert_eq!(
        store.eligibility(scope, now),
        QuotaFeedbackEligibility::Cooling
    );
    store.eligibility(scope, now + RETENTION);
    assert!(store.0.lock().unwrap().feedback.is_empty());
}

#[test]
fn a_live_guarded_probe_is_never_expired_by_the_legacy_ttl() {
    // Issue #1179 P2 repair: the builtin MCP guard owns the probe for its
    // whole in-flight lifetime, however long the attempt runs.
    let store = QuotaFeedbackStore::default();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
    let start = Instant::now();
    store.record_429(scope, Some(Duration::ZERO), start, start);
    let claim = store.claim_probe(scope, start);
    let QuotaProbeClaim::Reserved(guard) = claim else {
        panic!("the scope needs a probe");
    };
    assert_eq!(
        store.eligibility(scope, start),
        QuotaFeedbackEligibility::ProbeBusy
    );

    let far_later = start + PROBE_TTL + Duration::from_secs(600);
    assert_eq!(
        store.eligibility(scope, far_later),
        QuotaFeedbackEligibility::ProbeBusy,
        "a live guard is never expired by an eligibility read"
    );
    assert!(matches!(
        store.claim_probe(scope, far_later),
        QuotaProbeClaim::Busy
    ));
    assert!(
        !store.reserve_probe(scope, far_later),
        "the legacy reservation is also refused while a guard is live"
    );

    drop(guard);
    assert!(
        matches!(
            store.claim_probe(scope, far_later),
            QuotaProbeClaim::Reserved(_)
        ),
        "dropping the guard releases only its own claim"
    );
}

#[test]
fn a_stale_release_never_removes_a_newer_claim() {
    let store = QuotaFeedbackStore::default();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
    let start = Instant::now();
    store.record_429(scope, Some(Duration::ZERO), start, start);
    let claim = store.claim_probe(scope, start);
    let QuotaProbeClaim::Reserved(guard) = claim else {
        panic!("the scope needs a probe");
    };

    // A newer failure and an unrelated (older) release must not steal the
    // live guard's claim.
    let later = start + Duration::from_secs(5);
    store.record_failure(
        scope,
        QuotaFeedbackCause::Exhausted,
        Some(Duration::ZERO),
        later,
        later,
    );
    store.release_probe(scope, start - Duration::from_secs(1), later);

    assert_eq!(
        store.eligibility(scope, later),
        QuotaFeedbackEligibility::ProbeBusy,
        "the live guard still owns the probe"
    );
    drop(guard);
}

#[test]
fn the_unguarded_legacy_reservation_keeps_its_ttl_recovery() {
    let store = QuotaFeedbackStore::default();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
    let start = Instant::now();
    store.record_429(scope, Some(Duration::ZERO), start, start);
    assert!(store.reserve_probe(scope, start));
    assert!(
        !store.reserve_probe(scope, start),
        "an unguarded reservation still holds the slot inside the TTL"
    );
    assert!(
        store.reserve_probe(scope, start + PROBE_TTL),
        "an abandoned unguarded reservation expires after the TTL"
    );
}
