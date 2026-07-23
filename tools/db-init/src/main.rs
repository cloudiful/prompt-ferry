use anyhow::{Context, Result, anyhow};
use clap::Parser;
use db_init::{DatabaseUrlResolution, DbInitOptions, init_database, load_dotenv_if_exists};
use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

const DATABASE_URL_ENV_KEYS: &[&str] = &[
    "PROMPT_FERRY_DEV_DATABASE_URL",
    "PROMPT_FERRY_WORKER__DATABASE_URL",
    "DATABASE_URL",
];

#[derive(Debug, Parser)]
#[command(name = "db_init", about = "Database helpers for prompt-ferry")]
struct Cli {
    #[arg(long, env = "PROMPT_FERRY_DEV_DATABASE_URL")]
    database_url: Option<String>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("fatal: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    load_dotenv_if_exists(".env")?;
    let cli = Cli::parse();
    let resolution = resolve_database_url(cli.database_url)?;
    let _pool = init_database(
        &resolution.database_url,
        &MIGRATOR,
        DbInitOptions::default(),
    )
    .await?;
    println!("database init completed");
    Ok(())
}

fn resolve_database_url(from_arg: Option<String>) -> Result<DatabaseUrlResolution> {
    db_init::resolve_database_url(from_arg, DATABASE_URL_ENV_KEYS, || {
        let config = config::read::<serde_json::Value>(
            "prompt-ferry",
            Some(config::ReadOptions::with_env_prefix("PROMPT_FERRY")),
        )
        .context("failed to load prompt-ferry config")?;
        let worker = config
            .get("worker")
            .and_then(|value| value.get("database_url"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("worker.database_url is not configured"))?;
        Ok(Some(worker.to_string()))
    })
}
