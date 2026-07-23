use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "usage_replay_backfill",
    about = "Backfill missing assistant replay artifacts for prompt-ferry"
)]
struct Cli {
    #[arg(long, env = "PROMPT_FERRY_DEV_DATABASE_URL")]
    database_url: Option<String>,
    #[arg(long)]
    apply: bool,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("fatal: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    prompt_ferry::runtime_env::load_dotenv(".env")?;
    let cli = Cli::parse();
    let resolution = prompt_ferry::runtime_env::resolve_database_url(cli.database_url)?;
    let pool = prompt_ferry::db::connect(&resolution.database_url).await?;
    prompt_ferry::db::migrate(&pool).await?;

    let apply = cli.apply;
    let stats =
        prompt_ferry::chat_replay::backfill_missing_assistant_artifacts(&pool, apply).await?;
    println!(
        "scanned={} repaired={} skipped={} failed={} mode={}",
        stats.scanned,
        stats.repaired,
        stats.skipped,
        stats.failed,
        if apply { "apply" } else { "dry-run" }
    );
    Ok(())
}
