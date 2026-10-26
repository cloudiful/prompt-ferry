//! The endpoint key foreign key indexes: what they are, and what they must not
//! have changed.
//!
//! Two claims are kept apart on purpose. The first is about structure — an
//! index exists, PostgreSQL trusts it, and it leads with the foreign key column
//! — and that is asserted from the catalog alone. The second is about the
//! planner choosing that index for a lookup, and that is asserted against an
//! owned population in which the looked-up key is rare: the fixture writes rows
//! it owns that the lookup does not match, and refreshes the statistics of the
//! two tables it wrote, so the planner judges the data that is actually there
//! rather than whatever the last autovacuum happened to record.
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

use fk::{Owned, cleanup, index_leads_with, index_state, owned_endpoint, pool};
use usage::{
    PlanSummary, Table, analyze_table, count, explain_lookup, fill_conversation_overrides_n,
    fill_usage_charges_n, fixture_keys, rows_pointing_at,
};
use uuid::Uuid;

const USAGES_INDEX: &str = "idx_usage_charges_endpoint_key_id";
const OVERRIDES_INDEX: &str = "idx_conversation_endpoint_overrides_endpoint_key_id";

/// The rows the looked-up key matches.
///
/// Few enough that the index is the cheaper path, and counted against the rows
/// themselves rather than against a plan's own row count.
const PLAN_MATCHING_ROWS: i64 = 300;

/// The rows the fixture owns that the looked-up key does not match.
///
/// They are what makes the lookup selective. A table whose every row shares
/// one key value is answered correctly by a sequential scan, so the index
/// could never be the cheaper path there and the planner would be right to
/// ignore it.
const PLAN_DECOY_ROWS: i64 = 30_000;

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
async fn the_planner_prefers_the_key_index_for_a_selective_owned_lookup() -> anyhow::Result<()> {
    let pool = pool(2).await;
    let owned = owned_endpoint(&pool, "plan").await;
    // A second owned key carries the rows the lookup does not match. Without
    // them the owned key would match every row the fixture owns, and a
    // sequential scan would be the correct answer for that table.
    let decoy = owned_endpoint(&pool, "plan-decoy").await;

    // The measurement runs as its own task, so a failure inside it is
    // reported instead of unwinding past the cleanup: the fixture is removed
    // by its own ids first, and only then does the failure travel on.
    let measured = tokio::spawn({
        let owned = parts_of(&owned);
        let decoy = parts_of(&decoy);
        let pool = pool.clone();
        async move {
            let owned = Owned {
                endpoint_id: owned.0,
                key_id: owned.1,
                user_id: owned.2,
            };
            let decoy = Owned {
                endpoint_id: decoy.0,
                key_id: decoy.1,
                user_id: decoy.2,
            };
            measure_plan_case(&pool, &owned, &decoy).await
        }
    })
    .await;

    let measurements = match measured {
        Ok(Ok(measurements)) => measurements,
        Ok(Err(error)) => {
            cleanup(&pool, &owned).await;
            cleanup(&pool, &decoy).await;
            pool.close().await;
            return Err(error);
        }
        Err(join_error) => {
            cleanup(&pool, &owned).await;
            cleanup(&pool, &decoy).await;
            pool.close().await;
            std::panic::resume_unwind(join_error.into_panic());
        }
    };

    // The fixture owns every row it wrote, so it is removed by its own ids
    // before the assertions run. A failing assertion then cannot leave rows
    // behind for the next run to measure against.
    cleanup(&pool, &owned).await;
    cleanup(&pool, &decoy).await;
    pool.close().await;

    for measurement in &measurements {
        let table = measurement.table;
        assert_eq!(
            measurement.matching_rows,
            PLAN_MATCHING_ROWS,
            "{} must match exactly the fixture's own matching rows",
            table.name()
        );
        assert!(
            measurement.matching_rows * 100 <= measurement.owned_population,
            "{} matches {} of {} owned rows, which is not selective enough for \
             the index to be the cheaper path",
            table.name(),
            measurement.matching_rows,
            measurement.owned_population
        );
        assert!(
            measurement.owned_population >= PLAN_DECOY_ROWS,
            "{} owns only {} rows, too few for the planner's choice to say \
             anything about the index",
            table.name(),
            measurement.owned_population
        );
        // The plan is the claim: the row count is asserted above, against the
        // rows themselves, so what is left to assert here is the choice.
        assert!(
            measurement.plan.uses_index,
            "{} should use its endpoint_key_id index for a selective lookup, \
             chose {:?}",
            table.name(),
            measurement.plan.node_types
        );
    }
    Ok(())
}

/// What the lookup was measured against, read independently of the plan.
struct PlanMeasurement {
    table: Table,
    plan: PlanSummary,
    /// The rows that carry the looked-up key.
    matching_rows: i64,
    /// The rows the fixture owns, matching and not.
    owned_population: i64,
}

/// The owned endpoints' ids, in the form a task can carry.
fn parts_of(owned: &Owned) -> (Uuid, Uuid, i64) {
    (owned.endpoint_id, owned.key_id, owned.user_id)
}

/// Fill the owned population, refresh its statistics and read the plans.
///
/// Everything the assertions need is collected here so that the caller can
/// remove the fixture before it asserts anything.
async fn measure_plan_case(
    pool: &sqlx::PgPool,
    owned: &Owned,
    decoy: &Owned,
) -> anyhow::Result<Vec<PlanMeasurement>> {
    for table in [Table::Charges, Table::Overrides] {
        fill_table(pool, table, owned, PLAN_MATCHING_ROWS).await;
        fill_table(pool, table, decoy, PLAN_DECOY_ROWS).await;
    }

    // The statistics of the two tables the fixture wrote are refreshed, so the
    // planner judges the population that is actually there instead of the one
    // the last autovacuum happened to record.
    analyze_table(pool, Table::Charges).await?;
    analyze_table(pool, Table::Overrides).await?;

    let mut measurements = Vec::new();
    for table in [Table::Charges, Table::Overrides] {
        let plan = explain_lookup(pool, table, owned.key_id).await;
        // The row count comes from the rows, not from the plan: a plan's own
        // root count is the number of rows it returned, which is the same
        // number under either plan shape and so says nothing about the
        // fixture being complete.
        let matching_rows = rows_pointing_at(pool, table, owned.key_id).await;
        let owned_population = count(pool, table, owned.endpoint_id).await
            + count(pool, table, decoy.endpoint_id).await;
        measurements.push(PlanMeasurement {
            table,
            plan,
            matching_rows,
            owned_population,
        });
    }
    Ok(measurements)
}

/// Fill one of the two referencing tables with rows pointing at `owned`'s key.
async fn fill_table(pool: &sqlx::PgPool, table: Table, owned: &Owned, rows: i64) {
    match table {
        Table::Charges => {
            fill_usage_charges_n(pool, owned, rows).await;
        }
        Table::Overrides => {
            fill_conversation_overrides_n(pool, owned, rows).await;
        }
    }
}

#[tokio::test]
async fn deleting_a_key_nulls_both_references_and_keeps_the_rows() -> anyhow::Result<()> {
    let pool = pool(2).await;
    let owned = owned_endpoint(&pool, "setnull").await;
    // A second endpoint's rows, so the deletion can be shown to stay inside its
    // own fixture rather than merely appearing to.
    let neighbour = owned_endpoint(&pool, "neighbour").await;

    // As in the plan case, the work runs as its own task so that a failure
    // inside it is reported only after the fixture has been removed.
    let observed = tokio::spawn({
        let owned = parts_of(&owned);
        let neighbour = parts_of(&neighbour);
        let pool = pool.clone();
        async move {
            let owned = Owned {
                endpoint_id: owned.0,
                key_id: owned.1,
                user_id: owned.2,
            };
            let neighbour = Owned {
                endpoint_id: neighbour.0,
                key_id: neighbour.1,
                user_id: neighbour.2,
            };
            observe_set_null(&pool, &owned, &neighbour).await
        }
    })
    .await;

    let observations = match observed {
        Ok(Ok(observations)) => observations,
        Ok(Err(error)) => {
            cleanup(&pool, &owned).await;
            cleanup(&pool, &neighbour).await;
            pool.close().await;
            return Err(error);
        }
        Err(join_error) => {
            cleanup(&pool, &owned).await;
            cleanup(&pool, &neighbour).await;
            pool.close().await;
            std::panic::resume_unwind(join_error.into_panic());
        }
    };

    for observation in &observations.tables {
        let table = observation.table;
        assert_eq!(
            observation.still_pointing,
            0,
            "no {} row may still point at the deleted key",
            table.name()
        );
        assert_eq!(
            observation.kept,
            observation.before,
            "deleting a key must keep its {} rows, not cascade them away",
            table.name()
        );
        assert_eq!(
            observation.nulled,
            observation.before,
            "every {} row should now carry a null key reference",
            table.name()
        );
    }

    assert_eq!(
        observations.neighbour_pointing, observations.neighbour_charges,
        "the other endpoint's rows must be untouched"
    );
    assert_eq!(observations.neighbour_total, observations.neighbour_charges);
    assert_eq!(observations.remaining_keys, 0);
    Ok(())
}

/// What the deletion did, read before the fixture was removed.
struct SetNullObservations {
    tables: Vec<SetNullObservation>,
    neighbour_pointing: i64,
    neighbour_total: i64,
    neighbour_charges: i64,
    remaining_keys: i64,
}

/// What the deletion did to one of the two referencing tables.
struct SetNullObservation {
    table: Table,
    /// The rows that still carry the deleted key.
    still_pointing: i64,
    /// The rows that are still there.
    kept: i64,
    /// The rows that now carry no key.
    nulled: i64,
    /// The rows there were before the deletion.
    before: i64,
}

/// Delete the owned key and report what it did to the rows.
///
/// The counts are read before the fixture is removed, and the fixture is
/// removed before the counts are handed back, so the caller can assert on them
/// without the fixture's rows still being around to be asserted about.
async fn observe_set_null(
    pool: &sqlx::PgPool,
    owned: &Owned,
    neighbour: &Owned,
) -> anyhow::Result<SetNullObservations> {
    let charges = fill_usage_charges_n(pool, owned, 500).await;
    let overrides = fill_conversation_overrides_n(pool, owned, 500).await;
    let neighbour_charges = fill_usage_charges_n(pool, neighbour, 50).await;

    sqlx::query("DELETE FROM endpoint_api_keys WHERE key_id = $1")
        .bind(owned.key_id)
        .execute(pool)
        .await
        .expect("delete the owned key");

    let mut tables = Vec::new();
    for (table, before) in [(Table::Charges, charges), (Table::Overrides, overrides)] {
        tables.push(SetNullObservation {
            table,
            still_pointing: rows_pointing_at(pool, table, owned.key_id).await,
            kept: count(pool, table, owned.endpoint_id).await,
            nulled: nulled(pool, table, owned.endpoint_id).await,
            before,
        });
    }
    let neighbour_pointing = rows_pointing_at(pool, Table::Charges, neighbour.key_id).await;
    let neighbour_total = count(pool, Table::Charges, neighbour.endpoint_id).await;

    // The fixture is removed by its own ids before the results are handed
    // back, so a failing assertion cannot leave rows behind.
    cleanup(pool, owned).await;
    cleanup(pool, neighbour).await;
    let remaining_keys = fixture_keys(pool, owned.endpoint_id).await;

    Ok(SetNullObservations {
        tables,
        neighbour_pointing,
        neighbour_total,
        neighbour_charges,
        remaining_keys,
    })
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
