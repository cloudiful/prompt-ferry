use std::process::ExitCode;

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "usage_backfill",
    about = "Backfill historical request_records token fields and usage charges from \
             retained upstream raw payloads. Dry-run by default; pass --apply to write."
)]
struct Cli {
    #[arg(long, env = "PROMPT_FERRY_DEV_DATABASE_URL")]
    database_url: Option<String>,
    /// Write the repairs to PostgreSQL. Without this flag the tool only prints
    /// what it would change.
    #[arg(long, default_value_t = false)]
    apply: bool,
    /// Maximum rows inspected per batch. Each batch is a separate transaction.
    #[arg(long, default_value_t = 500)]
    limit: i64,
    /// Cap on the number of bounded batches the tool processes in one run.
    /// Use this to keep dry-runs small or to throttle a large production
    /// repair into reviewable chunks. Dry-run defaults to one batch; apply
    /// walks the cursor forward until the candidate set is exhausted.
    #[arg(long)]
    max_batches: Option<i64>,
    /// Inclusive lower bound on `request_records.created_at`.
    #[arg(long)]
    since: Option<DateTime<Utc>>,
    /// Exclusive upper bound on `request_records.created_at`.
    #[arg(long)]
    until: Option<DateTime<Utc>>,
    /// Key-set cursor: only rows with `event_id > after_event_id` are read.
    /// Defaults to `0` (start at the beginning). The CLI prints the cursor
    /// after each batch so an operator can resume a multi-batch run by hand.
    #[arg(long, default_value_t = 0)]
    after_event_id: i64,
    /// Cap on the number of diagnostic lines (event_id / decision / reason)
    /// emitted to stderr per batch. Defaults to 50 so a noisy batch cannot
    /// drown the operator's terminal. Set to `0` to silence diagnostics.
    #[arg(long, default_value_t = 50)]
    diagnostics_limit: usize,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("fatal: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    prompt_ferry::runtime_env::load_dotenv(".env")?;
    let cli = Cli::parse();
    let resolution = prompt_ferry::runtime_env::resolve_database_url(cli.database_url)?;
    let pool = prompt_ferry::db::connect(&resolution.database_url).await?;
    prompt_ferry::db::migrate(&pool).await?;

    // Dry-run defaults to a single bounded batch: the operator should review
    // the dry-run output before kicking off a multi-batch apply.
    let max_batches = cli
        .max_batches
        .unwrap_or(if cli.apply { i64::MAX } else { 1 })
        .max(1);
    let mut aggregate = prompt_ferry::db::BackfillStats::default();
    let mut cursor = cli.after_event_id.max(0);
    let mut batches_run: usize = 0;
    for batch_index in 0..max_batches {
        let options = prompt_ferry::db::BackfillOptions {
            apply: cli.apply,
            limit: cli.limit.max(1),
            since: cli.since,
            until: cli.until,
            after_event_id: cursor,
        };
        let outcome = prompt_ferry::db::backfill_token_usage(&pool, options).await?;
        batches_run = (batch_index + 1) as usize;
        let batch_scanned = outcome.stats.scanned;
        let next_cursor = outcome.last_event_id;
        eprintln!(
            "batch={} cursor_before={} cursor_after={} scanned={} repaired={} unchanged={} skipped={} failed={} diagnostics={}",
            batches_run,
            cursor,
            next_cursor,
            batch_scanned,
            outcome.stats.repaired,
            outcome.stats.unchanged,
            outcome.stats.skipped,
            outcome.stats.failed,
            outcome.diagnostics.len(),
        );
        // Emit per-event diagnostics to stderr so the operator can see the
        // exact event IDs that need follow-up. The cursor always advances so
        // a single failed/skipped row never wedges the run.
        emit_diagnostics(&outcome.diagnostics, cli.diagnostics_limit);
        aggregate.add(outcome.stats);
        if batch_scanned == 0 || batch_scanned < options.limit as usize {
            cursor = next_cursor;
            break;
        }
        cursor = next_cursor;
        if !cli.apply {
            // Dry-run is a one-shot review; subsequent batches would only
            // repeat the same candidates.
            break;
        }
    }
    println!(
        "mode={} batches_run={} scanned={} repaired={} unchanged={} skipped={} failed={} cursor_after={}",
        if cli.apply { "apply" } else { "dry-run" },
        batches_run,
        aggregate.scanned,
        aggregate.repaired,
        aggregate.unchanged,
        aggregate.skipped,
        aggregate.failed,
        cursor,
    );
    if aggregate.failed > 0 && cli.apply {
        // Per-event failures roll back inside the backfill transaction so the
        // database stays consistent, but the operator still needs a non-zero
        // exit code to react to a partial run.
        return Ok(ExitCode::from(2));
    }
    Ok(ExitCode::SUCCESS)
}

fn emit_diagnostics(diagnostics: &[prompt_ferry::db::BackfillOutcome], limit: usize) {
    if limit == 0 || diagnostics.is_empty() {
        return;
    }
    let visible = diagnostics.len().min(limit);
    eprintln!(
        "diagnostics[showing={visible}/total={}]:",
        diagnostics.len()
    );
    for outcome in diagnostics.iter().take(visible) {
        match &outcome.reason {
            Some(reason) => eprintln!(
                "  event_id={} decision={} reason={}",
                outcome.event_id,
                outcome.decision.label(),
                reason
            ),
            None => eprintln!(
                "  event_id={} decision={} reason=<none>",
                outcome.event_id,
                outcome.decision.label()
            ),
        }
    }
    if diagnostics.len() > visible {
        eprintln!(
            "  ...({} more diagnostics suppressed)",
            diagnostics.len() - visible
        );
    }
}
