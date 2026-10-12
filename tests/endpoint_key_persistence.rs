//! Endpoint key persistence against the confirmed PostgreSQL test database.
//!
//! A save used to rewrite every key it was given: positions were shifted for
//! the whole set, everything unrequested was deleted, and then each requested
//! key was updated whether or not it had changed. These tests pin what a save
//! writes instead — an unchanged key keeps its `updated_at` and its row, an
//! edited key is the only one rewritten, a reorder moves positions without
//! pretending to be an edit, and a request that cannot be stored leaves the
//! endpoint exactly as it was.

#[path = "support/endpoint_key_persistence.rs"]
mod persistence;

use prompt_ferry::db::{self, EndpointApiKeyCreate};
use sqlx::PgPool;
use uuid::Uuid;

use persistence::{cleanup, endpoint_input, endpoints_named, key, pool, stored_keys, unique_name};

/// Create an endpoint and hand back its id once the test is finished with it.
async fn endpoint_with(pool: &PgPool, name: &str, keys: Vec<EndpointApiKeyCreate>) -> Uuid {
    let created = db::create_endpoint(pool, endpoint_input(name, keys))
        .await
        .expect("create the fixture endpoint");
    created.endpoint_id
}

#[tokio::test]
async fn a_repeated_save_leaves_every_key_row_untouched() -> anyhow::Result<()> {
    let pool = pool(1).await;
    let name = unique_name("keys-noop");
    let endpoint_id = endpoint_with(
        &pool,
        &name,
        vec![key(None, "primary", 0, true), key(None, "backup", 1, true)],
    )
    .await;

    let before = stored_keys(&pool, endpoint_id).await;
    let input = endpoint_input(
        &name,
        vec![
            key(Some(before[0].key_id), "primary", 0, true),
            key(Some(before[1].key_id), "backup", 1, true),
        ],
    );

    db::update_endpoint(&pool, endpoint_id, input).await?;

    let after = stored_keys(&pool, endpoint_id).await;
    assert_eq!(after.len(), 2);
    for (was, now) in before.iter().zip(after.iter()) {
        assert_eq!(now.key_id, was.key_id, "a save must not replace a key");
        assert_eq!(
            now.updated_at, was.updated_at,
            "an unchanged key must keep its updated_at"
        );
        assert_eq!(
            now.created_at, was.created_at,
            "a save must never reset a key's creation time"
        );
        assert_eq!(now.position, was.position, "positions must not drift");
    }
    cleanup(&pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn editing_one_field_rewrites_only_that_key() -> anyhow::Result<()> {
    let pool = pool(1).await;
    let name = unique_name("keys-edit");
    let endpoint_id = endpoint_with(
        &pool,
        &name,
        vec![key(None, "primary", 0, true), key(None, "backup", 1, true)],
    )
    .await;
    let before = stored_keys(&pool, endpoint_id).await;

    // Disable only the first key, and change nothing else.
    let input = endpoint_input(
        &name,
        vec![
            key(Some(before[0].key_id), "primary", 0, false),
            key(Some(before[1].key_id), "backup", 1, true),
        ],
    );
    db::update_endpoint(&pool, endpoint_id, input).await?;

    let after = stored_keys(&pool, endpoint_id).await;
    assert!(!after[0].enabled, "the edited key must carry the new flag");
    assert!(
        after[0].updated_at > before[0].updated_at,
        "an edit must advance that key's updated_at"
    );
    assert_eq!(
        after[1].updated_at, before[1].updated_at,
        "the untouched key must not be rewritten"
    );
    assert_eq!(
        after[1].created_at, before[1].created_at,
        "the untouched key keeps its creation time"
    );
    cleanup(&pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn a_request_without_key_ids_keeps_the_stored_keys() -> anyhow::Result<()> {
    let pool = pool(1).await;
    let name = unique_name("keys-legacy");
    let endpoint_id = endpoint_with(
        &pool,
        &name,
        vec![key(None, "primary", 0, true), key(None, "backup", 1, true)],
    )
    .await;
    let before = stored_keys(&pool, endpoint_id).await;

    // A client that sends labels but no ids still means the keys it names, so
    // the stored identity — and with it the creation time — has to survive.
    let input = endpoint_input(
        &name,
        vec![key(None, "primary", 0, true), key(None, "backup", 1, true)],
    );
    db::update_endpoint(&pool, endpoint_id, input).await?;

    let after = stored_keys(&pool, endpoint_id).await;
    assert_eq!(after[0].key_id, before[0].key_id);
    assert_eq!(after[1].key_id, before[1].key_id);
    for (was, now) in before.iter().zip(after.iter()) {
        assert_eq!(now.updated_at, was.updated_at);
        assert_eq!(now.created_at, was.created_at);
    }
    cleanup(&pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn reordering_moves_positions_and_leaves_the_keys_unaltered() -> anyhow::Result<()> {
    let pool = pool(1).await;
    let name = unique_name("keys-reorder");
    let endpoint_id = endpoint_with(
        &pool,
        &name,
        vec![
            key(None, "primary", 0, true),
            key(None, "backup", 1, true),
            key(None, "spare", 2, true),
        ],
    )
    .await;
    let before = stored_keys(&pool, endpoint_id).await;

    // Swap the two ends: a full cycle, which is where a naive position update
    // collides with the unique index.
    let input = endpoint_input(
        &name,
        vec![
            key(Some(before[2].key_id), "spare", 0, true),
            key(Some(before[1].key_id), "backup", 1, true),
            key(Some(before[0].key_id), "primary", 2, true),
        ],
    );
    db::update_endpoint(&pool, endpoint_id, input).await?;

    let after = stored_keys(&pool, endpoint_id).await;
    assert_eq!(
        after
            .iter()
            .map(|k| k.key_label.as_str())
            .collect::<Vec<_>>(),
        vec!["spare", "backup", "primary"]
    );
    assert_eq!(
        after.iter().map(|k| k.position).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "positions must be the requested ones, with no offset left behind"
    );
    for (was, now) in before.iter().zip(after.iter()) {
        // Reordering is not an edit, so no timestamp may move.
        assert_eq!(
            now.updated_at, was.updated_at,
            "{} was rewritten",
            now.key_label
        );
        assert_eq!(now.created_at, was.created_at);
    }
    cleanup(&pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn replacing_a_key_keeps_the_survivors_and_ages_the_rest() -> anyhow::Result<()> {
    let pool = pool(1).await;
    let name = unique_name("keys-swap");
    let endpoint_id = endpoint_with(
        &pool,
        &name,
        vec![key(None, "primary", 0, true), key(None, "backup", 1, true)],
    )
    .await;
    let before = stored_keys(&pool, endpoint_id).await;

    // Drop the backup and put a new key in the position it held.
    let input = endpoint_input(
        &name,
        vec![
            key(Some(before[0].key_id), "primary", 0, true),
            key(None, "spare", 1, true),
        ],
    );
    db::update_endpoint(&pool, endpoint_id, input).await?;

    let after = stored_keys(&pool, endpoint_id).await;
    assert_eq!(
        after
            .iter()
            .map(|k| k.key_label.as_str())
            .collect::<Vec<_>>(),
        vec!["primary", "spare"]
    );
    assert_eq!(
        after[0].key_id, before[0].key_id,
        "the surviving key keeps its identity"
    );
    assert_eq!(
        after[0].created_at, before[0].created_at,
        "the surviving key keeps its creation time"
    );
    assert_ne!(after[1].key_id, before[1].key_id, "the new key is new");
    cleanup(&pool, endpoint_id).await;
    Ok(())
}

#[tokio::test]
async fn a_refused_create_leaves_no_endpoint_behind() -> anyhow::Result<()> {
    let pool = pool(1).await;
    let name = unique_name("keys-create-fail");

    let input = endpoint_input(
        &name,
        vec![key(None, "primary", 0, true), key(None, "backup", 0, true)],
    );
    let outcome = db::create_endpoint(&pool, input).await;
    assert!(
        outcome.is_err(),
        "two keys may not share a position, so the create must be refused"
    );

    assert_eq!(
        endpoints_named(&pool, &name).await,
        0,
        "a refused create must not leave an unreferenced endpoint behind"
    );
    Ok(())
}
