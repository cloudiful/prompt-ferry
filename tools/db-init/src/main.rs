use anyhow::{Context, Result, bail};
use clap::Parser;
use prompt_ferry::config::{load_repository_env, read_node_config};
use prompt_ferry::db::gate::{NodeGate, RuntimeIdentity};
use std::time::Duration;
use uuid::Uuid;

/// Upper bound for the gate claim and the migration it owns.
const GATE_PREPARE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Parser)]
#[command(
    name = "db_init",
    about = "Prepare the prompt-ferry database through the node gate"
)]
struct Cli {
    /// Acknowledge that pre-node processes are stopped so the gate may cut
    /// over a legacy schema. The gate only asks for it when it detects one.
    #[arg(long, env = "PROMPT_FERRY_ACK_STOP_OLD_NODES")]
    ack_stop_old_nodes: bool,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("fatal: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    load_repository_env().context("failed to load the .env file")?;
    let cli = Cli::parse();
    let config = read_node_config().context("failed to read the node configuration")?;
    let database_url = config.database_url.trim();
    if database_url.is_empty() {
        bail!("the node configuration has no database URL");
    }
    let identity = RuntimeIdentity::current().context("failed to resolve the runtime identity")?;
    let gate = NodeGate::prepare_database(
        database_url,
        identity,
        Uuid::new_v4(),
        cli.ack_stop_old_nodes,
        GATE_PREPARE_TIMEOUT,
    )
    .await
    .context("failed to prepare the node database")?;
    println!(
        "database init completed: runtime {}.{}.{}, protocol {}",
        gate.identity.version.0,
        gate.identity.version.1,
        gate.identity.version.2,
        gate.identity.protocol
    );
    Ok(())
}
