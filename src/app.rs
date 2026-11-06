use crate::{
    certs,
    cli::{Cli, Command, OpenapiCommand},
    config, openapi_export, serve, worker,
};
use anyhow::Context;
use clap::Parser;
use std::path::Path;
use tracing::{debug, info};
use tracing_subscriber::{
    EnvFilter, Layer, Registry, filter::FilterFn, fmt, layer::SubscriberExt,
    util::SubscriberInitExt,
};

const DEFAULT_LOG_FILTER: &str = "info,rmcp::service=warn";

/// Targets that may log the raw authorization code, token response or other
/// OAuth material. They are denied at the layer level, so no `RUST_LOG` value
/// can re-enable them. The personal MCP OAuth backend itself only emits its
/// own sanitized `category`/`event` fields.
const DENIED_LOG_TARGET_PREFIXES: &[&str] =
    &["rmcp::transport::auth", "rmcp::transport::common::auth"];

fn is_denied_log_target(target: &str) -> bool {
    DENIED_LOG_TARGET_PREFIXES
        .iter()
        .any(|prefix| target == *prefix || target.starts_with(&format!("{prefix}::")))
}

/// Build the effective subscriber filter.
///
/// `RUST_LOG` still controls ordinary verbosity, but the OAuth internals stay
/// off because the deny filter is applied on top of the `EnvFilter` rather than
/// as another directive it could override.
fn env_filter(level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(format!("{level},rmcp::service=warn")))
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER))
}

fn logging_layer(level: &str) -> impl Layer<Registry> + Send + Sync {
    fmt::layer()
        .with_filter(env_filter(level))
        .with_filter(FilterFn::new(|metadata: &tracing::Metadata<'_>| {
            !is_denied_log_target(metadata.target())
        }))
}

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
    // The level is embedded in the default filter so `RUST_LOG` still wins for
    // ordinary targets; the OAuth deny filter is applied on top of it and can
    // never be re-enabled from the environment.
    let _ = tracing_subscriber::registry()
        .with(logging_layer(level))
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::{is_denied_log_target, logging_layer};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::Layer;
    use tracing_subscriber::fmt::MakeWriter;
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Capture {
        fn contents(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("capture")).to_string()
        }
    }

    impl Write for Capture {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("capture").extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Capture;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn oauth_internal_targets_are_denied_while_service_targets_stay_enabled() {
        assert!(is_denied_log_target("rmcp::transport::auth"));
        assert!(is_denied_log_target("rmcp::transport::auth::enterprise"));
        assert!(is_denied_log_target("rmcp::transport::common::auth"));
        assert!(!is_denied_log_target("rmcp::service"));
        assert!(!is_denied_log_target("prompt_ferry::mcp::oauth::flow"));
    }

    #[test]
    fn oauth_authorization_material_never_reaches_the_log_even_with_trace_verbosity() {
        let capture = Capture::default();
        // Simulate `RUST_LOG=trace`: the deny filter is applied on top of an
        // allow-everything `EnvFilter`, so the OAuth internals still stay off.
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .with_writer(capture.clone())
                .with_filter(tracing_subscriber::filter::EnvFilter::new("trace"))
                .with_filter(tracing_subscriber::filter::FilterFn::new(
                    |metadata: &tracing::Metadata<'_>| !is_denied_log_target(metadata.target()),
                )),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(
                target: "rmcp::transport::auth",
                "start exchange code for token: raw-code and vendor-id-token"
            );
            tracing::info!(
                target: "prompt_ferry::mcp::oauth",
                event = "authorization_started",
                "personal MCP OAuth authorization started"
            );
        });
        let rendered = capture.contents();
        assert!(
            !rendered.contains("raw-code"),
            "the authorization code must never be logged: {rendered}"
        );
        assert!(
            !rendered.contains("vendor-id-token"),
            "vendor token material must never be logged: {rendered}"
        );
        assert!(
            rendered.contains("authorization_started"),
            "sanitized service events stay visible: {rendered}"
        );
        let _ = logging_layer("info");
    }
}
