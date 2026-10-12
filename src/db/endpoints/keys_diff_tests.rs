//! The planner decides, without a database, which rows a save has to touch.
//! These cases pin that decision; the timing and rollback behaviour of the
//! statements it produces is covered against a real endpoint store.

use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use super::keys_diff::{RENUMBER_OFFSET, plan_key_persistence};
use crate::db::types::{EndpointApiKey, EndpointApiKeyCreate};

const ENDPOINT: Uuid = Uuid::from_u128(0x1000);

fn stored(key_id: u128, label: &str, secret: &str, position: i32, enabled: bool) -> EndpointApiKey {
    EndpointApiKey {
        key_id: Uuid::from_u128(key_id),
        endpoint_id: ENDPOINT,
        key_label: label.to_string(),
        api_key: secret.to_string(),
        position,
        enabled,
        created_at: stamp(0),
        updated_at: stamp(0),
    }
}

fn stamp(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .expect("valid timestamp")
}

fn requested(
    key_id: Option<u128>,
    label: &str,
    secret: &str,
    position: i32,
    enabled: bool,
) -> EndpointApiKeyCreate {
    EndpointApiKeyCreate {
        key_label: label.to_string(),
        api_key: secret.to_string(),
        position,
        enabled,
        key_id: key_id.map(Uuid::from_u128),
    }
}

/// The message a refused request reports.
///
/// A plan carries secrets, so it deliberately does not implement `Debug` and
/// `expect_err` cannot be used on a result that holds one.
fn refusal(current: &[EndpointApiKey], incoming: &[EndpointApiKeyCreate]) -> String {
    match plan_key_persistence(current, incoming) {
        Ok(_) => panic!("the request should have been refused"),
        Err(err) => err.to_string(),
    }
}

fn two_keys() -> Vec<EndpointApiKey> {
    vec![
        stored(1, "primary", "secret-one", 0, true),
        stored(2, "backup", "secret-two", 1, true),
    ]
}

#[test]
fn saving_the_same_key_set_plans_nothing() {
    let current = two_keys();
    let plan = plan_key_persistence(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, true),
            requested(Some(2), "backup", "secret-two", 1, true),
        ],
    )
    .expect("a repeated save is valid");

    // Nothing is written, so no `updated_at` advances and no position shifts.
    assert!(plan.rewrites.is_empty());
    assert!(plan.repositions.is_empty());
    assert!(plan.inserts.is_empty());
    assert!(plan.removed.is_empty());
    assert!(plan.is_empty());
}

#[test]
fn editing_one_field_rewrites_only_that_key() {
    let current = two_keys();
    let plan = plan_key_persistence(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, false),
            requested(Some(2), "backup", "secret-two", 1, true),
        ],
    )
    .expect("a valid edit");

    assert_eq!(plan.rewrites.len(), 1);
    assert_eq!(plan.rewrites[0].key_id, Uuid::from_u128(1));
    assert!(!plan.rewrites[0].enabled);
    // The untouched key is not rewritten and is not renumbered either.
    assert!(plan.repositions.is_empty());
    assert!(plan.inserts.is_empty());
    assert!(plan.removed.is_empty());
}

#[test]
fn renaming_a_secret_leaves_the_key_identity_in_place() {
    let current = two_keys();
    let plan = plan_key_persistence(
        &current,
        &[
            requested(Some(1), "primary", "rotated-secret", 0, true),
            requested(Some(2), "backup", "secret-two", 1, true),
        ],
    )
    .expect("a valid rotation");

    assert_eq!(plan.rewrites.len(), 1);
    assert_eq!(plan.rewrites[0].key_id, Uuid::from_u128(1));
    assert_eq!(plan.rewrites[0].api_key, "rotated-secret");
}

#[test]
fn reordering_moves_positions_without_rewriting_the_key() {
    let current = two_keys();
    let plan = plan_key_persistence(
        &current,
        &[
            requested(Some(2), "backup", "secret-two", 0, true),
            requested(Some(1), "primary", "secret-one", 1, true),
        ],
    )
    .expect("a valid reorder");

    // Reordering is not an edit: it must not advance `updated_at`.
    assert!(plan.rewrites.is_empty());
    assert_eq!(
        plan.repositions,
        vec![(Uuid::from_u128(2), 0), (Uuid::from_u128(1), 1)]
    );
}

#[test]
fn a_label_without_an_id_keeps_the_stored_key() {
    let current = two_keys();
    let plan = plan_key_persistence(
        &current,
        &[
            requested(None, "backup", "secret-two", 0, true),
            requested(None, "primary", "secret-one", 1, true),
        ],
    )
    .expect("a legacy reorder is valid");

    // A request that names no id is matched by label, so the keys keep the
    // identity and creation time they already had.
    assert!(plan.inserts.is_empty());
    assert!(plan.rewrites.is_empty());
    assert_eq!(
        plan.repositions,
        vec![(Uuid::from_u128(2), 0), (Uuid::from_u128(1), 1)]
    );
    assert_eq!(plan.retained, vec![Uuid::from_u128(2), Uuid::from_u128(1)]);
}

#[test]
fn a_new_key_is_inserted_and_a_dropped_key_is_removed() {
    let current = two_keys();
    let plan = plan_key_persistence(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, true),
            requested(None, "spare", "secret-three", 1, true),
        ],
    )
    .expect("a swap is valid");

    assert_eq!(plan.removed, vec![Uuid::from_u128(2)]);
    assert_eq!(plan.retained, vec![Uuid::from_u128(1)]);
    assert_eq!(plan.inserts.len(), 1);
    assert_eq!(plan.inserts[0].key_label, "spare");
    assert_eq!(plan.inserts[0].position, 1);
    // The survivor keeps its position, so there is nothing to renumber.
    assert!(plan.repositions.is_empty());
}

#[test]
fn dropping_every_key_plans_only_a_removal() {
    let current = two_keys();
    let plan = plan_key_persistence(&current, &[]).expect("an empty key set is valid");

    assert_eq!(plan.removed.len(), 2);
    assert!(plan.retained.is_empty());
    assert!(plan.inserts.is_empty());
}

#[test]
fn the_reorder_band_stays_clear_of_requested_positions() {
    let current = vec![stored(1, "primary", "secret-one", 0, true)];
    // A position inside the reserved band would be ambiguous: the writer shifts
    // a moving key into exactly that range to get it out of the way.
    let far = RENUMBER_OFFSET;
    let err = refusal(
        &current,
        &[requested(Some(1), "primary", "secret-one", far, true)],
    );

    assert!(err.contains("reorder band"), "{err}");
}

#[test]
fn a_position_with_no_room_to_reorder_is_refused() {
    // A stored key already at the top of the range leaves the shift unable to
    // land, which would otherwise wrap into a position someone else holds.
    let current = vec![stored(1, "primary", "secret-one", i32::MAX, true)];
    let err = refusal(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, true),
            requested(None, "second", "secret-two", 1, true),
        ],
    );

    assert!(err.contains("no room"), "{err}");
}

#[test]
fn two_requests_for_one_stored_key_are_refused() {
    let current = two_keys();
    let err = refusal(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, true),
            requested(None, "primary", "secret-one", 1, true),
        ],
    );

    assert!(err.contains("requested twice"), "{err}");
}

#[test]
fn two_requests_for_one_position_are_refused() {
    let current = two_keys();
    let err = refusal(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, true),
            requested(Some(2), "backup", "secret-two", 0, true),
        ],
    );

    assert!(err.contains("position 0 is requested twice"), "{err}");
}

#[test]
fn an_id_that_is_no_longer_stored_is_treated_as_a_new_key() {
    let current = two_keys();
    // The id names a row a concurrent save already replaced. The request still
    // means the key it labelled, so it is inserted rather than refused or —
    // worse — matched to nothing and silently lost.
    let plan = plan_key_persistence(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, true),
            requested(Some(99), "backup", "secret-two", 1, true),
        ],
    )
    .expect("a stale id falls back to the label");

    // The label still matches the stored backup, so that key keeps its identity.
    assert!(plan.inserts.is_empty());
    assert_eq!(plan.retained, vec![Uuid::from_u128(1), Uuid::from_u128(2)]);
}

#[test]
fn a_stale_id_for_an_unknown_label_is_inserted() {
    let current = two_keys();
    let plan = plan_key_persistence(
        &current,
        &[
            requested(Some(1), "primary", "secret-one", 0, true),
            requested(Some(99), "spare", "secret-three", 1, true),
        ],
    )
    .expect("a stale id for a new label is a new key");

    assert_eq!(plan.inserts.len(), 1);
    assert_eq!(plan.inserts[0].key_label, "spare");
    assert_eq!(plan.retained, vec![Uuid::from_u128(1)]);
}
