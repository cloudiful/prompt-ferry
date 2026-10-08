use crate::{
    certs,
    cli::{Cli, Command, OpenapiCommand},
    config, openapi_export, serve, worker,
};
use anyhow::Context;
use clap::Parser;
use std::path::Path;
use tracing::{debug, info};
use tracing_subscriber::{EnvFilter, fmt};

const DEFAULT_LOG_FILTER: &str = "info,rmcp::service=warn";

pub async fn run_cli() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
    let cli = Cli::parse();

    // The repository `.env` is read as data before any configuration is read,
    // so a plain `prompt-ferry` picks up local values without a shell
    // wrapper. Values already present in the process environment are kept, and
    // CLI arguments still win over both.
    let dotenv_path = config::load_repository_env().context("failed to load the .env file")?;

    let (integrated, entrypoint) = match cli.command {
        None => (
            Some(cli.integrated.clone()),
            serve::Entrypoint::NoSubcommand,
        ),
        Some(Command::Serve(args)) => (Some(args), serve::Entrypoint::ServeAlias),
        Some(command) => {
            run_component(command).await?;
            return Ok(());
        }
    };
    let args = integrated.expect("the integrated path is selected above");
    let app_config = config::read_app_config().context("failed to read config")?;
    init_logging(&app_config.logging.level);
    log_dotenv(dotenv_path.as_deref());
    serve::run(app_config, args, entrypoint).await
}

async fn run_component(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Openapi(command) => match command {
            OpenapiCommand::Export(args) => openapi_export::export_admin_api(&args.out),
        },
        Command::Cert(command) => match command {
            crate::cli::CertCommand::Init(args) => certs::init(args),
        },
        Command::Relay(args) => {
            let app_config = config::read_app_config().context("failed to read config")?;
            init_logging(&app_config.logging.level);
            serve::run_relay_command(app_config, args).await
        }
        Command::Worker(args) => {
            let app_config = config::read_app_config().context("failed to read config")?;
            init_logging(&app_config.logging.level);
            worker::run(app_config.worker.merge_args(args)).await
        }
        // Both integrated spellings are handled by the caller.
        Command::Serve(_) => Ok(()),
    }
}

/// Name the dotenv file that was read. Only the path is reported; a value
/// from it may be a database URL or a token and never reaches the log.
fn log_dotenv(path: Option<&Path>) {
    match path {
        Some(path) => info!(env_file = %path.display(), "loaded environment values from .env"),
        None => debug!("no .env file found; using the process environment and built-in defaults"),
    }
}

fn init_logging(level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(format!("{level},rmcp::service=warn")))
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    let _ = fmt().with_env_filter(filter).try_init();
}
