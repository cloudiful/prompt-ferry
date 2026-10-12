//! What an endpoint key deletion costs on this database, before and after the
//! foreign key indexes exist.
//!
//! This is a measurement, not a gate. It builds one owned key, fills both
//! referencing tables with rows that point at it, and explains two statements:
//! the `ON DELETE SET NULL` work a key deletion triggers, and a plain lookup on
//! the foreign key column. The delete is explained inside a transaction that is
//! rolled back, so no row is actually removed.
//!
//! Run it before the migration and again after. It prints the numbers and states
//! which indexes were present when it ran, so a second run cannot be mistaken
//! for a first one. Nothing here asserts a duration: a timing assertion would
//! turn a CI machine's mood into a test failure.

#[allow(dead_code)]
#[path = "support/endpoint_key_fk_fixtures.rs"]
mod fk;
#[allow(dead_code)]
#[path = "support/endpoint_key_fk_usage_fixtures.rs"]
mod usage;

use sqlx::PgPool;

use fk::{IndexState, Owned, cleanup, index_state, owned_endpoint, pool};
use usage::{
    PlanSummary, Table, count, explain_key_delete_in, explain_lookup, fill_conversation_overrides,
    fill_usage_charges, fixture_keys,
};

const USAGES_INDEX: &str = "idx_usage_charges_endpoint_key_id";
const OVERRIDES_INDEX: &str = "idx_conversation_endpoint_overrides_endpoint_key_id";

#[tokio::test]
async fn measure_key_deletion_cost() -> anyhow::Result<()> {
    let pool = pool(2).await;
    let owned = owned_endpoint(&pool, "baseline").await;
    let charges = fill_usage_charges(&pool, &owned).await;
    let overrides = fill_conversation_overrides(&pool, &owned).await;
    report(
        "fixture",
        &format!("{charges} charges, {overrides} overrides"),
    );

    // Recorded, not asserted: the point is that the report says which state it
    // measured, so two runs can be compared honestly.
    report(
        "indexes present",
        &format!(
            "{USAGES_INDEX}={}, {OVERRIDES_INDEX}={}",
            describe(&index_state(&pool, USAGES_INDEX).await),
            describe(&index_state(&pool, OVERRIDES_INDEX).await)
        ),
    );

    let delete = delete_plan(&pool, &owned).await;
    report(
        "delete endpoint_api_keys (SET NULL trigger)",
        &format!(
            "exec {:.3}ms, plan {:.3}ms, top rows {:.0}, nodes {:?}, index scan {}",
            delete.execution_ms,
            delete.planning_ms,
            delete.total_rows,
            delete.node_types,
            delete.uses_index
        ),
    );

    for table in [Table::Charges, Table::Overrides] {
        let lookup = explain_lookup(&pool, table, owned.key_id).await;
        report(
            &format!("lookup {}.endpoint_key_id", table_name(table)),
            &format!(
                "exec {:.3}ms, rows {:.0}, nodes {:?}, index scan {}",
                lookup.execution_ms, lookup.total_rows, lookup.node_types, lookup.uses_index
            ),
        );
    }

    // The deletion was explained, not performed: the rollback has to have put
    // everything back, or this measurement would be lying about what it measured.
    assert_eq!(
        count(&pool, Table::Charges, owned.endpoint_id).await,
        charges,
        "the rollback must restore the fixture charges"
    );
    assert_eq!(
        count(&pool, Table::Overrides, owned.endpoint_id).await,
        overrides,
        "the rollback must restore the fixture overrides"
    );
    report("rollback", "fixture rows and owned key intact");

    cleanup(&pool, &owned).await;
    assert_eq!(
        leftovers(&pool, &owned).await,
        0,
        "the fixture left rows behind"
    );
    report("cleanup", "0 rows left behind");
    Ok(())
}

/// Explain the deletion of the owned key inside a transaction that is undone.
///
/// The statement really runs, so the `ON DELETE SET NULL` work really happens —
/// that is the cost being recorded — and the rollback really puts it back.
async fn delete_plan(pool: &PgPool, owned: &Owned) -> PlanSummary {
    let mut tx = pool.begin().await.expect("begin the benchmark transaction");
    // `SET` takes no bind parameters, so the timeout is a literal here.
    sqlx::query("SET LOCAL statement_timeout = '5s'")
        .execute(&mut *tx)
        .await
        .expect("bound the benchmark statement");
    let plan = explain_key_delete_in(&mut tx, owned.key_id).await;
    tx.rollback().await.expect("roll the benchmark back");
    plan
}

/// Any row of ours that survived the cleanup.
async fn leftovers(pool: &PgPool, owned: &Owned) -> i64 {
    count(pool, Table::Charges, owned.endpoint_id).await
        + count(pool, Table::Overrides, owned.endpoint_id).await
        + fixture_keys(pool, owned.endpoint_id).await
}

fn table_name(table: Table) -> &'static str {
    match table {
        Table::Charges => "usage_charges",
        Table::Overrides => "conversation_endpoint_overrides",
    }
}

fn describe(state: &Option<IndexState>) -> String {
    match state {
        None => "absent".to_string(),
        Some(state) => format!(
            "present valid={} ready={} [{}]",
            state.valid, state.ready, state.definition
        ),
    }
}

/// Printed rather than asserted: the recorded numbers are the deliverable.
fn report(what: &str, value: &str) {
    println!("[fk-baseline] {what}: {value}");
}
