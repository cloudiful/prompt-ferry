//! The endpoint key foreign key indexes: what they are, and what they must not
//! have changed.
//!
//! Two claims are kept apart on purpose. The first is about structure — an
//! index exists, PostgreSQL trusts it, and it leads with the foreign key column
//! — and that is asserted. The second is about the planner choosing it, and
//! that is asserted only under a large enough owned fixture for the choice to
//! be about the index rather than about the table being tiny.
//!
//! The other half is what the index must leave alone. The foreign keys are
//! `ON DELETE SET NULL`, so deleting a key has to keep the rows and clear their
//! reference. These tests delete an owned key for real and check that.

#[allow(dead_code)]
#[path = "support/endpoint_key_fk_fixtures.rs"]
mod fk;
#[allow(dead_code)]
#[path = "support/endpoint_key_fk_usage_fixtures.rs"]
mod usage;

use std::fs;

use fk::{cleanup, index_leads_with, index_state, owned_endpoint, pool};
use usage::{
    Table, count, explain_lookup, fill_conversation_overrides_n, fill_usage_charges_n,
    fixture_keys, rows_pointing_at,
};
use uuid::Uuid;

const USAGES_INDEX: &str = "idx_usage_charges_endpoint_key_id";
const OVERRIDES_INDEX: &str = "idx_conversation_endpoint_overrides_endpoint_key_id";

/// Enough rows that using the index is cheaper than scanning, which is the
/// volume at which the planner's choice says something about the index.
const PLAN_ROWS: i64 = 30_000;

#[test]
fn each_migration_is_one_non_transactional_concurrent_statement() {
    // PostgreSQL refuses `CONCURRENTLY` inside a transaction block, and sqlx
    // sends a migration file as a single batch, so a file holding two of these
    // statements cannot apply at all. That is why there are two migrations
    // rather than one, and this is the assertion that keeps it that way.
    let pairs = [
        (
            "20261012025132_endpoint_key_fk_usage_charges_endpoint_key_id",
            USAGES_INDEX,
        ),
        (
            "20261012025141_endpoint_key_fk_conversation_endpoint_overrides_endpoint_key_id",
            OVERRIDES_INDEX,
        ),
    ];

    for (stem, index_name) in pairs {
        for direction in ["up", "down"] {
            let path = format!(
                "{}/migrations/{stem}.{direction}.sql",
                env!("CARGO_MANIFEST_DIR")
            );
            let sql = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
            // A statement can wrap across lines, so the body is reassembled
            // before it is counted; only `;` ends one.
            let body = sql
                .lines()
                .filter(|line| !line.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join(" ");
            let statements = body
                .split(';')
                .map(str::trim)
                .filter(|statement| !statement.is_empty())
                .collect::<Vec<_>>();

            assert_eq!(
                statements.len(),
                1,
                "{stem}.{direction}.sql holds {} statements; a multi-statement \
                 batch is an implicit transaction block and CONCURRENTLY is refused in one",
                statements.len()
            );
            let statement = statements[0];
            assert!(
                statement.contains("CONCURRENTLY"),
                "{stem}.{direction}.sql must build or drop the index concurrently: {statement}"
            );
            assert!(
                sql.starts_with("-- no-transaction"),
                "{stem}.{direction}.sql must open with the sqlx directive, or sqlx wraps it in \
                 a transaction and the statement cannot run"
            );
            assert!(
                statement.contains(index_name),
                "{stem}.{direction}.sql must name {index_name}, found: {statement}"
            );
        }
    }
}

#[tokio::test]
async fn both_indexes_are_present_trusted_and_lead_with_the_key_column() -> anyhow::Result<()> {
    let pool = pool(1).await;

    for (index_name, table) in [
        (USAGES_INDEX, Table::Charges),
        (OVERRIDES_INDEX, Table::Overrides),
    ] {
        let state = index_state(&pool, index_name)
            .await
            .unwrap_or_else(|| panic!("{index_name} does not exist on {} ", table.name()));
        // An index left invalid by a failed concurrent build still has a name
        // in the catalog but is never used. Counting names would miss that.
        assert!(
            state.valid,
            "{index_name} exists but is not valid: {}",
            state.definition
        );
        assert!(
            state.ready,
            "{index_name} exists but is not ready: {}",
            state.definition
        );
        assert!(
            index_leads_with(&pool, index_name, "endpoint_key_id").await,
            "{index_name} must lead with endpoint_key_id, found: {}",
            state.definition
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_large_owned_fixture_makes_the_key_lookups_index_eligible() -> anyhow::Result<()> {
    let pool = pool(2).await;
    let owned = owned_endpoint(&pool, "plan").await;
    fill_usage_charges_n(&pool, &owned, PLAN_ROWS).await;
    fill_conversation_overrides_n(&pool, &owned, PLAN_ROWS).await;

    for table in [Table::Charges, Table::Overrides] {
        let plan = explain_lookup(&pool, table, owned.key_id).await;
        // The row count is asserted rather than the duration: the plan is the
        // claim, the milliseconds are context.
        assert_eq!(
            plan.total_rows,
            PLAN_ROWS as f64,
            "{} should match every fixture row",
            table.name()
        );
        assert!(
            plan.uses_index,
            "{} at {PLAN_ROWS} rows should use its endpoint_key_id index, chose {:?}",
            table.name(),
            plan.node_types
        );
    }

    cleanup(&pool, &owned).await;
    Ok(())
}

#[tokio::test]
async fn deleting_a_key_nulls_both_references_and_keeps_the_rows() -> anyhow::Result<()> {
    let pool = pool(2).await;
    let owned = owned_endpoint(&pool, "setnull").await;
    let charges = fill_usage_charges_n(&pool, &owned, 500).await;
    let overrides = fill_conversation_overrides_n(&pool, &owned, 500).await;

    // A second endpoint's rows, so the deletion can be shown to stay inside its
    // own fixture rather than merely appearing to.
    let neighbour = owned_endpoint(&pool, "neighbour").await;
    let neighbour_charges = fill_usage_charges_n(&pool, &neighbour, 50).await;

    sqlx::query("DELETE FROM endpoint_api_keys WHERE key_id = $1")
        .bind(owned.key_id)
        .execute(&pool)
        .await
        .expect("delete the owned key");

    for (table, before) in [(Table::Charges, charges), (Table::Overrides, overrides)] {
        assert_eq!(
            rows_pointing_at(&pool, table, owned.key_id).await,
            0,
            "no {} row may still point at the deleted key",
            table.name()
        );
        assert_eq!(
            count(&pool, table, owned.endpoint_id).await,
            before,
            "deleting a key must keep its {} rows, not cascade them away",
            table.name()
        );
        assert_eq!(
            nulled(&pool, table, owned.endpoint_id).await,
            before,
            "every {} row should now carry a null key reference",
            table.name()
        );
    }

    assert_eq!(
        rows_pointing_at(&pool, Table::Charges, neighbour.key_id).await,
        neighbour_charges,
        "the other endpoint's rows must be untouched"
    );
    assert_eq!(
        count(&pool, Table::Charges, neighbour.endpoint_id).await,
        neighbour_charges
    );

    cleanup(&pool, &owned).await;
    cleanup(&pool, &neighbour).await;
    assert_eq!(fixture_keys(&pool, owned.endpoint_id).await, 0);
    Ok(())
}

/// How many of the fixture's rows now carry no key reference.
async fn nulled(pool: &sqlx::PgPool, table: Table, endpoint_id: Uuid) -> i64 {
    let counted: i64 = match table {
        Table::Charges => sqlx::query_scalar(
            "SELECT count(*) FROM usage_charges WHERE endpoint_id = $1 AND endpoint_key_id IS NULL",
        )
        .bind(endpoint_id)
        .fetch_one(pool)
        .await
        .expect("count the nulled charges"),
        Table::Overrides => sqlx::query_scalar(
            "SELECT count(*) FROM conversation_endpoint_overrides \
             WHERE endpoint_id = $1 AND endpoint_key_id IS NULL",
        )
        .bind(endpoint_id)
        .fetch_one(pool)
        .await
        .expect("count the nulled overrides"),
    };
    counted
}
