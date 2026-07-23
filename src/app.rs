use crate::{
    certs,
    cli::{Cli, Command, OpenapiCommand},
    config, openapi_export, relay, serve, worker,
};
use anyhow::Context;
use clap::Parser;
use tracing_subscriber::{EnvFilter, fmt};

const DEFAULT_LOG_FILTER: &str = "info,rmcp::service=warn";

pub async fn run_cli() -> anyhow::Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");
    let cli = Cli::parse();

    match cli.command {
        Command::Openapi(command) => match command {
            OpenapiCommand::Export(args) => openapi_export::export_admin_api(&args.out),
        },
        Command::Cert(command) => match command {
            crate::cli::CertCommand::Init(args) => certs::init(args),
        },
        Command::Relay(args) => {
            let app_config = config::read_app_config().context("failed to read config")?;
            init_logging(&app_config.logging.level);
            relay::run(app_config.relay.merge_args(args)).await
        }
        Command::Worker(args) => {
            let app_config = config::read_app_config().context("failed to read config")?;
            init_logging(&app_config.logging.level);
            worker::run(app_config.worker.merge_args(args)).await
        }
        Command::Serve(args) => {
            let app_config = config::read_app_config().context("failed to read config")?;
            init_logging(&app_config.logging.level);
            serve::run(app_config, args).await
        }
    }
}

fn init_logging(level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(format!("{level},rmcp::service=warn")))
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    let _ = fmt().with_env_filter(filter).try_init();
}
