use super::{ProbeClaim, claim_at};
use crate::worker_admin::quota_feedback::{QuotaFeedbackScope, QuotaFeedbackStore};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[test]
fn expired_cooldown_allows_one_owned_probe_and_drop_releases_it() {
    let store = QuotaFeedbackStore::default();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), Some(Uuid::new_v4()));
    let failed_at = Instant::now();
    store.record_429(scope, None, failed_at, failed_at);
    let probe_at = failed_at + Duration::from_secs(31);

    let ProbeClaim::Reserved(permit) = claim_at(&store, scope, probe_at) else {
        panic!("the expired cooldown should admit one probe");
    };
    assert!(matches!(
        claim_at(&store, scope, probe_at),
        ProbeClaim::LostRace
    ));
    drop(permit);
    assert!(matches!(
        claim_at(&store, scope, probe_at + Duration::from_millis(1)),
        ProbeClaim::Reserved(_)
    ));
}

#[test]
fn cloned_permit_releases_only_once_after_last_owner_or_explicit_release() {
    let store = QuotaFeedbackStore::default();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), None);
    let failed_at = Instant::now();
    store.record_429(scope, None, failed_at, failed_at);
    let ProbeClaim::Reserved(permit) = claim_at(&store, scope, failed_at + Duration::from_secs(31))
    else {
        panic!("the expired cooldown should admit a probe");
    };
    let clone = permit.clone();
    permit.release();
    drop(permit);
    assert!(matches!(
        claim_at(
            &store,
            scope,
            failed_at + Duration::from_secs(31) + Duration::from_millis(1)
        ),
        ProbeClaim::Reserved(_)
    ));
    drop(clone);
}
