//! Two saves of one endpoint must not interleave.
//!
//! The key set is read and then written. Without a lock on the endpoint row,
//! two overlapping saves both read the same starting rows and both write a key
//! set derived from them, so the endpoint ends up holding a mixture of the two
//! requests that neither caller asked for. The lock is what stops it, so the
//! first test takes the row lock directly and shows a save cannot get past it.

#[path = "support/endpoint_key_persistence.rs"]
mod persistence;

use std::time::Duration;

use prompt_ferry::db;
use sqlx::PgPool;
use uuid::Uuid;

use persistence::{
    cleanup, endpoint_input, key, pool, stored_endpoint_name, stored_keys, unique_name,
};

/// Two separate pools, so a lock held on one is held by a different connection.
async fn two_pools() -> (PgPool, PgPool) {
    (pool(1).await, pool(1).await)
}

/// Long enough that a save blocked on the row lock has clearly not finished,
/// short enough not to make the suite slow. This bounds a proof, it is not a
/// benchmark: what it shows is that the save does not complete while the lock
/// is held, not how long it would take.
const WHILE_LOCKED: Duration = Duration::from_millis(500);

/// Create an endpoint with three keys and return its id.
async fn seeded(pool: &PgPool, name: &str) -> Uuid {
    let created = db::create_endpoint(
        pool,
        endpoint_input(
            name,
            vec![
                key(None, "primary", 0, true),
                key(None, "backup", 1, true),
                key(None, "spare", 2, true),
            ],
        ),
    )
    .await
    .expect("create the fixture endpoint");
    created.endpoint_id
}

/// The stored labels in position order.
async fn shape(pool: &PgPool, endpoint_id: Uuid) -> Vec<String> {
    stored_keys(pool, endpoint_id)
        .await
        .into_iter()
        .map(|key| key.key_label)
        .collect()
}

#[tokio::test]
async fn a_save_waits_for_the_endpoint_row_lock_before_it_reads_the_keys() -> anyhow::Result<()> {
    // The holder keeps a transaction open while this test reads through the
    // same pool, so that pool needs a connection beyond the one it holds.
    let holder_pool = pool(2).await;
    let writer_pool = pool(1).await;
    let name = unique_name("keys-lock");
    let endpoint_id = seeded(&holder_pool, &name).await;
    let before = stored_keys(&holder_pool, endpoint_id).await;

    // Take the parent row's lock the way a concurrent save would, and keep it.
    let mut holder = holder_pool.begin().await?;
    sqlx::query("SELECT endpoint_id FROM provider_endpoints WHERE endpoint_id = $1 FOR UPDATE")
        .bind(endpoint_id)
        .fetch_one(&mut *holder)
        .await?;

    let request = endpoint_input(
        &name,
        vec![
            key(Some(before[0].key_id), "primary", 0, false),
            key(Some(before[1].key_id), "backup", 1, true),
        ],
    );
    let saving =
        tokio::spawn(async move { db::update_endpoint(&writer_pool, endpoint_id, request).await });

    // While the row is locked the save cannot get as far as reading the keys,
    // so the stored set is untouched and the save is still running.
    tokio::time::sleep(WHILE_LOCKED).await;
    assert!(
        !saving.is_finished(),
        "the save completed while the endpoint row was locked by another transaction"
    );
    assert_eq!(
        shape(&holder_pool, endpoint_id).await,
        vec!["primary", "backup", "spare"],
        "a save waiting for the lock must not have written anything"
    );

    holder.commit().await?;
    let saved = saving.await??;
    assert!(
        saved.is_some(),
        "the save should find the endpoint once it proceeds"
    );

    let landed = stored_keys(&holder_pool, endpoint_id).await;
    assert_eq!(
        landed
            .iter()
            .map(|k| k.key_label.as_str())
            .collect::<Vec<_>>(),
        vec!["primary", "backup"],
        "the save lands its own whole key set once the lock is released"
    );
    assert!(!landed[0].enabled, "the save's edit must be applied");
    assert_eq!(
        landed[0].key_id, before[0].key_id,
        "the key keeps its identity across a lock wait"
    );

    cleanup(&holder_pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn overlapping_saves_leave_one_whole_key_set() -> anyhow::Result<()> {
    let (first_pool, second_pool) = two_pools().await;
    let name = unique_name("keys-overlap");
    let endpoint_id = seeded(&first_pool, &name).await;
    let before = stored_keys(&first_pool, endpoint_id).await;

    // Two genuinely different requests for the same endpoint: the first drops
    // the spare key, the second drops the primary and adds a fourth.
    let first_request = endpoint_input(
        &name,
        vec![
            key(Some(before[0].key_id), "primary", 0, true),
            key(Some(before[1].key_id), "backup", 1, true),
        ],
    );
    let second_request = endpoint_input(
        &name,
        vec![
            key(Some(before[1].key_id), "backup", 0, true),
            key(Some(before[2].key_id), "spare", 1, true),
            key(None, "extra", 2, true),
        ],
    );

    let writer = first_pool.clone();
    let first =
        tokio::spawn(async move { db::update_endpoint(&writer, endpoint_id, first_request).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let writer = second_pool.clone();
    let second =
        tokio::spawn(
            async move { db::update_endpoint(&writer, endpoint_id, second_request).await },
        );
    first.await??;
    second.await??;

    // Whichever save committed last, the endpoint holds one whole requested
    // set — never a mixture of the two.
    let landed = shape(&first_pool, endpoint_id).await;
    let first_shape = vec!["primary".to_string(), "backup".to_string()];
    let second_shape = vec![
        "backup".to_string(),
        "spare".to_string(),
        "extra".to_string(),
    ];
    assert!(
        landed == first_shape || landed == second_shape,
        "endpoint ended on an interleaved key set: {landed:?}"
    );

    // Positions follow the labels, so the surviving set is internally
    // consistent and no reorder offset was left behind.
    let stored = stored_keys(&first_pool, endpoint_id).await;
    for (index, key) in stored.iter().enumerate() {
        assert_eq!(
            key.position, index as i32,
            "{} was stored at the wrong position",
            key.key_label
        );
    }

    cleanup(&first_pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn a_refused_save_under_contention_adds_nothing() -> anyhow::Result<()> {
    let (first_pool, second_pool) = two_pools().await;
    let name = unique_name("keys-contention");
    let endpoint_id = seeded(&first_pool, &name).await;
    let before = stored_keys(&first_pool, endpoint_id).await;

    let good = endpoint_input(
        &name,
        vec![key(Some(before[0].key_id), "primary", 0, false)],
    );
    let invalid = endpoint_input(
        &name,
        vec![
            key(Some(before[0].key_id), "primary", 0, true),
            // Claims the same stored key a second time, so this save cannot be
            // stored however it is scheduled.
            key(Some(before[0].key_id), "primary", 1, true),
        ],
    );

    let writer = first_pool.clone();
    let good_save =
        tokio::spawn(async move { db::update_endpoint(&writer, endpoint_id, good).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let writer = second_pool.clone();
    let bad_save =
        tokio::spawn(async move { db::update_endpoint(&writer, endpoint_id, invalid).await });

    let bad = bad_save.await?;
    assert!(bad.is_err(), "the invalid save must be refused");
    good_save.await??;

    // Whichever order they took, the refused save left nothing of its own
    // behind: the endpoint holds exactly the committed save's key.
    let landed = stored_keys(&second_pool, endpoint_id).await;
    assert_eq!(landed.len(), 1, "the refused save must add no key");
    assert_eq!(landed[0].key_label, "primary");

    cleanup(&second_pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn a_key_requested_twice_leaves_the_endpoint_as_it_was() -> anyhow::Result<()> {
    let pool = pool(1).await;
    let name = unique_name("keys-duplicate");
    let endpoint_id = db::create_endpoint(
        &pool,
        endpoint_input(
            &name,
            vec![key(None, "primary", 0, true), key(None, "backup", 1, true)],
        ),
    )
    .await
    .expect("create the fixture endpoint")
    .endpoint_id;
    let before = stored_keys(&pool, endpoint_id).await;

    let input = endpoint_input(
        &format!("{name}-renamed"),
        vec![
            key(Some(before[0].key_id), "primary", 0, true),
            // The same stored key claimed a second time.
            key(Some(before[0].key_id), "primary", 1, true),
        ],
    );
    let outcome = db::update_endpoint(&pool, endpoint_id, input).await;
    assert!(
        outcome.is_err(),
        "a request that claims one key twice must be refused"
    );

    // The endpoint rename is part of the same save, so it has to roll back too.
    assert_eq!(
        stored_endpoint_name(&pool, endpoint_id).await.as_deref(),
        Some(name.as_str()),
        "the endpoint row must be unchanged after a refused save"
    );
    let after = stored_keys(&pool, endpoint_id).await;
    assert_eq!(
        after
            .iter()
            .map(|k| k.key_label.as_str())
            .collect::<Vec<_>>(),
        vec!["primary", "backup"],
        "the stored key set must be unchanged"
    );
    for (was, now) in before.iter().zip(after.iter()) {
        assert_eq!(now.key_id, was.key_id);
        assert_eq!(now.updated_at, was.updated_at);
    }
    cleanup(&pool, endpoint_id).await;
    Ok(())
}
