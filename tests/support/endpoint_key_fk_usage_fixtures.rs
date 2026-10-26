//! Volume for the endpoint key FK index tests.
//!
//! A lookup on a foreign key column is only interesting once the table holds
//! enough rows that scanning it is visibly different from using an index, so
//! these helpers fill both referencing tables with rows that point at one owned
//! key. Everything is written with the fixture's own endpoint id, which is what
//! cleanup deletes by, so no row outlives the run.
//!
//! Every statement is a literal with a bound parameter. The tests read rows
//! from two tables, and spelling both out is cheaper than composing a name into
//! SQL — the alternative is a formatted query string, which is exactly the kind
//! that is worth making impossible to construct by accident.

use sqlx::PgPool;
use uuid::Uuid;

use crate::fk::Owned;

/// How many rows each referencing table gets.
///
/// Bounded rather than large: the point is a representative lookup, and the
/// fixture is deleted again the moment the measurement is taken. The behaviour
/// tests pass their own, smaller counts, so this is only read by the baseline.
#[allow(dead_code)]
pub const FILLER_ROWS: i64 = 30_000;

/// The two tables that reference an endpoint key.
#[derive(Clone, Copy)]
pub enum Table {
    Charges,
    Overrides,
}

impl Table {
    pub fn name(self) -> &'static str {
        match self {
            Table::Charges => "usage_charges",
            Table::Overrides => "conversation_endpoint_overrides",
        }
    }
}

/// Insert [`FILLER_ROWS`] charges pointing at the owned key.
///
/// All of them reference one key on purpose. A `ON DELETE SET NULL` trigger has
/// to find every row that references the row being deleted, so the cost being
/// measured is "find N rows that share a key" — the lookup the index is for —
/// and not "scan a table where the key happens to appear once".
pub async fn fill_usage_charges(pool: &PgPool, owned: &Owned) -> i64 {
    fill_usage_charges_n(pool, owned, FILLER_ROWS).await
}

/// As [`fill_usage_charges`], with the row count chosen by the caller.
pub async fn fill_usage_charges_n(pool: &PgPool, owned: &Owned, rows: i64) -> i64 {
    sqlx::query(
        "INSERT INTO usage_charges \
           (request_id, user_id, endpoint_id, endpoint_key_id, usage_status, pricing_status) \
         SELECT gen_random_uuid(), $1, $2, $3, 'known', 'priced' FROM generate_series(1, $4)",
    )
    .bind(owned.user_id)
    .bind(owned.endpoint_id)
    .bind(owned.key_id)
    .bind(rows)
    .execute(pool)
    .await
    .expect("insert the fixture charges");
    count(pool, Table::Charges, owned.endpoint_id).await
}

/// Insert [`FILLER_ROWS`] conversation overrides pointing at the owned key.
///
/// The primary key is the conversation id, so each row needs its own.
pub async fn fill_conversation_overrides(pool: &PgPool, owned: &Owned) -> i64 {
    fill_conversation_overrides_n(pool, owned, FILLER_ROWS).await
}

/// As [`fill_conversation_overrides`], with the row count chosen by the caller.
pub async fn fill_conversation_overrides_n(pool: &PgPool, owned: &Owned, rows: i64) -> i64 {
    sqlx::query(
        "INSERT INTO conversation_endpoint_overrides \
           (conversation_id, endpoint_id, created_by_user_id, endpoint_key_id) \
         SELECT gen_random_uuid(), $1, $2, $3 FROM generate_series(1, $4)",
    )
    .bind(owned.endpoint_id)
    .bind(owned.user_id)
    .bind(owned.key_id)
    .bind(rows)
    .execute(pool)
    .await
    .expect("insert the fixture overrides");
    count(pool, Table::Overrides, owned.endpoint_id).await
}

/// How many rows of `table` belong to the fixture.
pub async fn count(pool: &PgPool, table: Table, endpoint_id: Uuid) -> i64 {
    let counted: i64 = match table {
        Table::Charges => {
            sqlx::query_scalar("SELECT count(*) FROM usage_charges WHERE endpoint_id = $1")
                .bind(endpoint_id)
                .fetch_one(pool)
                .await
                .expect("count the fixture charges")
        }
        Table::Overrides => sqlx::query_scalar(
            "SELECT count(*) FROM conversation_endpoint_overrides WHERE endpoint_id = $1",
        )
        .bind(endpoint_id)
        .fetch_one(pool)
        .await
        .expect("count the fixture overrides"),
    };
    counted
}

/// How many rows of `table` still point at `key_id`.
///
/// Used after a deletion to show the `ON DELETE SET NULL` behaviour itself is
/// untouched: the rows are still there, they are just no longer tied to a key.
pub async fn rows_pointing_at(pool: &PgPool, table: Table, key_id: Uuid) -> i64 {
    let counted: i64 = match table {
        Table::Charges => {
            sqlx::query_scalar("SELECT count(*) FROM usage_charges WHERE endpoint_key_id = $1")
                .bind(key_id)
                .fetch_one(pool)
                .await
                .expect("count the charges referencing the key")
        }
        Table::Overrides => sqlx::query_scalar(
            "SELECT count(*) FROM conversation_endpoint_overrides WHERE endpoint_key_id = $1",
        )
        .bind(key_id)
        .fetch_one(pool)
        .await
        .expect("count the overrides referencing the key"),
    };
    counted
}

/// How many keys the fixture's endpoint holds, for the cleanup assertion.
pub async fn fixture_keys(pool: &PgPool, endpoint_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM endpoint_api_keys WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .fetch_one(pool)
        .await
        .expect("count the fixture keys")
}

/// Refresh the planner's statistics for one of the two referencing tables.
///
/// `ANALYZE` names a table and cannot bind it, so the name is composed from
/// the closed [`Table`] list and nothing a caller supplies reaches the
/// statement. The production code does the same for a partition it has just
/// created (`src/db/partition_ddl.rs`): statistics are an input the planner
/// needs, so a fixture refreshes them for the population it actually wrote
/// instead of leaving the outcome to whichever autovacuum run happened to
/// happen first.
pub async fn analyze_table(pool: &PgPool, table: Table) -> anyhow::Result<()> {
    let statement = format!("ANALYZE {}", table.name());
    sqlx::query(sqlx::AssertSqlSafe(statement))
        .execute(pool)
        .await?;
    Ok(())
}

/// The plan PostgreSQL chose, reduced to what a test can assert on.
#[derive(Debug)]
pub struct PlanSummary {
    pub node_types: Vec<String>,
    pub uses_index: bool,
    pub planning_ms: f64,
    pub execution_ms: f64,
    pub total_rows: f64,
}

/// Explain a lookup on the foreign key column of `table`.
///
/// `EXPLAIN ANALYZE` runs the statement. For a lookup that only reads, so it is
/// harmless. The bound value makes the planner produce a custom plan, which is
/// the plan the deletion's `SET NULL` trigger actually gets to work with.
pub async fn explain_lookup(pool: &PgPool, table: Table, key_id: Uuid) -> PlanSummary {
    let raw: serde_json::Value = match table {
        Table::Charges => sqlx::query_scalar(
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) \
             SELECT * FROM usage_charges WHERE endpoint_key_id = $1",
        )
        .bind(key_id)
        .fetch_one(pool)
        .await
        .expect("explain the charges lookup"),
        Table::Overrides => sqlx::query_scalar(
            "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) \
             SELECT * FROM conversation_endpoint_overrides WHERE endpoint_key_id = $1",
        )
        .bind(key_id)
        .fetch_one(pool)
        .await
        .expect("explain the overrides lookup"),
    };
    summarize(raw)
}

/// Explain the deletion of one key on the caller's transaction.
///
/// This really deletes, because `EXPLAIN ANALYZE` runs its statement. Running it
/// on the caller's transaction is what lets a statement timeout apply and the
/// work be rolled back afterwards.
pub async fn explain_key_delete_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key_id: Uuid,
) -> PlanSummary {
    let raw: serde_json::Value =
        sqlx::query_scalar(sqlx::AssertSqlSafe(KEY_DELETE_EXPLAIN.to_string()))
            .bind(key_id)
            .fetch_one(&mut **tx)
            .await
            .expect("explain the key deletion");
    summarize(raw)
}

/// `EXPLAIN` only accepts a statement, never a prepared one, so the text is
/// built here and its value stays a bound parameter.
const KEY_DELETE_EXPLAIN: &str =
    "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) DELETE FROM endpoint_api_keys WHERE key_id = $1";

/// Reduce the `FORMAT JSON` plan document to what a test can assert on.
pub fn summarize(raw: serde_json::Value) -> PlanSummary {
    let plan = &raw[0]["Plan"];
    let mut node_types = Vec::new();
    let mut uses_index = false;
    collect(plan, &mut node_types, &mut uses_index);

    PlanSummary {
        node_types,
        uses_index,
        planning_ms: raw[0]["Planning Time"].as_f64().unwrap_or_default(),
        execution_ms: raw[0]["Execution Time"].as_f64().unwrap_or_default(),
        total_rows: plan["Actual Rows"].as_f64().unwrap_or_default(),
    }
}

fn collect(node: &serde_json::Value, types: &mut Vec<String>, uses_index: &mut bool) {
    if let Some(kind) = node["Node Type"].as_str() {
        types.push(kind.to_string());
    }
    // A bitmap index scan is an index scan: the planner fetches the matching
    // tids first and then visits the heap in physical order. Reading only the
    // two plain names would report an indexed lookup as unindexed, which is
    // exactly the mistake that would hide a regression in the other direction.
    if matches!(
        node["Node Type"].as_str(),
        Some("Index Scan" | "Index Only Scan" | "Bitmap Index Scan")
    ) {
        *uses_index = true;
    }
    if let Some(children) = node.get("Plans").and_then(|plans| plans.as_array()) {
        for entry in children {
            collect(entry, types, uses_index);
        }
    }
}
