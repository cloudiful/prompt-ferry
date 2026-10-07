use crate::config::{BridgeEncryptionMode, McpWarmupMode, NativeApi, TlsMode, WorkerTlsMode};
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "prompt-ferry",
    version,
    about = "OpenAI-compatible relay and worker for upstream ferrying",
    long_about = "Run with no subcommand to start the integrated mode: the relay, the worker, and \
                  the admin UI/API in one process.\n\n`serve` is a compatibility alias for the same \
                  integrated startup. The `relay`, `worker`, `openapi`, and `cert` subcommands \
                  start their component on its own.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Integrated-mode options. Applies to the no-subcommand startup; the
    /// `serve` alias carries its own copy of the same flag.
    #[command(flatten)]
    pub integrated: ServeArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Relay(RelayArgs),
    Worker(WorkerArgs),
    Serve(ServeArgs),
    #[command(subcommand)]
    Openapi(OpenapiCommand),
    #[command(subcommand)]
    Cert(CertCommand),
}

#[derive(Debug, Subcommand)]
pub enum OpenapiCommand {
    Export(ExportOpenapiArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ExportOpenapiArgs {
    #[arg(long, default_value = "openapi/admin-api.yaml")]
    pub out: String,
}

#[derive(Debug, Args, Clone, Default)]
pub struct RelayArgs {
    #[arg(long)]
    pub bind: Option<String>,
    #[arg(long)]
    pub worker_bind: Option<String>,
    #[arg(long)]
    pub client_token: Option<String>,
    #[arg(long)]
    pub worker_token: Option<String>,
    #[arg(long)]
    pub request_timeout_seconds: Option<u64>,
    #[arg(long)]
    pub worker_heartbeat_timeout_seconds: Option<u64>,
    #[arg(long)]
    pub response_stream_buffer: Option<usize>,
    #[arg(long)]
    pub response_stream_max_bytes: Option<usize>,
    #[arg(long)]
    pub response_stream_backpressure_timeout_ms: Option<u64>,
    #[arg(long)]
    pub tls_mode: Option<TlsMode>,
    #[arg(long)]
    pub tls_cert: Option<String>,
    #[arg(long)]
    pub tls_key: Option<String>,
    #[arg(long)]
    pub tls_client_ca: Option<String>,
    #[arg(long)]
    pub worker_tls_mode: Option<TlsMode>,
    #[arg(long)]
    pub worker_tls_cert: Option<String>,
    #[arg(long)]
    pub worker_tls_key: Option<String>,
    #[arg(long)]
    pub worker_tls_client_ca: Option<String>,
    #[arg(long)]
    pub bridge_encryption_mode: Option<BridgeEncryptionMode>,
    #[arg(long)]
    pub bridge_encryption_key: Option<String>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct WorkerArgs {
    #[arg(long)]
    pub relay_url: Vec<String>,
    #[arg(long)]
    pub worker_token: Option<String>,
    #[arg(long)]
    pub upstream_base_url: Option<String>,
    #[arg(long)]
    pub upstream_api_key: Option<String>,
    #[arg(long)]
    pub upstream_native_api: Option<NativeApi>,
    #[arg(long)]
    pub connect_timeout_seconds: Option<u64>,
    #[arg(long)]
    pub admin_bind: Option<String>,
    #[arg(long)]
    pub database_url: Option<String>,
    #[arg(long)]
    pub standalone_database_path: Option<String>,
    #[arg(long)]
    pub bootstrap_admin_login: Option<String>,
    #[arg(long)]
    pub bootstrap_admin_password: Option<String>,
    #[arg(long)]
    pub tls_mode: Option<WorkerTlsMode>,
    #[arg(long)]
    pub relay_ca: Option<String>,
    #[arg(long)]
    pub client_cert: Option<String>,
    #[arg(long)]
    pub client_key: Option<String>,
    #[arg(long)]
    pub bridge_encryption_mode: Option<BridgeEncryptionMode>,
    #[arg(long)]
    pub bridge_encryption_key: Option<String>,
    #[arg(long)]
    pub valkey_url: Option<String>,
    #[arg(long)]
    pub valkey_ttl_seconds: Option<u64>,
    #[arg(long)]
    pub session_ttl_seconds: Option<u64>,
    #[arg(long)]
    pub local_session_max_entries: Option<usize>,
    #[arg(long)]
    pub max_upstream_response_bytes: Option<usize>,
    #[arg(long)]
    pub max_raw_response_capture_bytes: Option<usize>,
    #[arg(long)]
    pub max_response_text_capture_bytes: Option<usize>,
    #[arg(long)]
    pub endpoint_model_cache_ttl_seconds: Option<u64>,
    #[arg(long)]
    pub shutdown_drain_seconds: Option<u64>,
    #[arg(long)]
    pub mcp_warmup: Option<McpWarmupMode>,
    #[arg(long)]
    pub mcp_allowed_origins: Vec<String>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct ServeArgs {
    /// Loopback address for the in-process relay/worker bridge. Use port `0`
    /// (or an empty value) to let prompt-ferry select a free loopback port.
    #[arg(long)]
    pub internal_worker_bind: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum CertCommand {
    Init(CertInitArgs),
}

#[derive(Debug, Args, Clone)]
pub struct CertInitArgs {
    #[arg(long)]
    pub host: String,
    #[arg(long, default_value = "./certs")]
    pub out: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn no_subcommand_selects_the_integrated_startup() {
        let cli = Cli::parse_from(["prompt-ferry"]);

        assert!(
            cli.command.is_none(),
            "an argument-free invocation must be the integrated entrypoint"
        );
        assert_eq!(cli.integrated.internal_worker_bind, None);
    }

    #[test]
    fn integrated_options_parse_without_a_subcommand() {
        let cli = Cli::parse_from(["prompt-ferry", "--internal-worker-bind", "127.0.0.1:0"]);

        assert!(cli.command.is_none());
        assert_eq!(
            cli.integrated.internal_worker_bind.as_deref(),
            Some("127.0.0.1:0")
        );
    }

    #[test]
    fn serve_alias_parses_its_own_option() {
        let cli = Cli::parse_from([
            "prompt-ferry",
            "serve",
            "--internal-worker-bind",
            "127.0.0.1:9788",
        ]);

        let Some(Command::Serve(alias)) = cli.command else {
            panic!("expected the serve compatibility alias");
        };
        assert_eq!(
            alias.internal_worker_bind.as_deref(),
            Some("127.0.0.1:9788")
        );
    }

    #[test]
    fn an_integrated_option_may_not_be_combined_with_a_subcommand() {
        // Mixing them would silently drop the flag, so the parser refuses it.
        Cli::try_parse_from([
            "prompt-ferry",
            "--internal-worker-bind",
            "127.0.0.1:0",
            "relay",
        ])
        .expect_err("an integrated option must not be applied to another subcommand");
    }

    #[test]
    fn serve_alias_without_options_keeps_the_built_in_bridge_bind() {
        let cli = Cli::parse_from(["prompt-ferry", "serve"]);

        match cli.command {
            Some(Command::Serve(alias)) => assert_eq!(alias.internal_worker_bind, None),
            other => panic!("expected the serve compatibility alias, got {other:?}"),
        }
    }

    #[test]
    fn component_subcommands_are_unchanged() {
        for (argv, expected) in [
            (
                vec!["prompt-ferry", "relay", "--bind", "127.0.0.1:9000"],
                Command::Relay(RelayArgs {
                    bind: Some("127.0.0.1:9000".to_string()),
                    ..RelayArgs::default()
                }),
            ),
            (
                vec!["prompt-ferry", "worker", "--admin-bind", "127.0.0.1:9001"],
                Command::Worker(WorkerArgs {
                    admin_bind: Some("127.0.0.1:9001".to_string()),
                    ..WorkerArgs::default()
                }),
            ),
            (
                vec!["prompt-ferry", "openapi", "export"],
                Command::Openapi(OpenapiCommand::Export(ExportOpenapiArgs {
                    out: "openapi/admin-api.yaml".to_string(),
                })),
            ),
        ] {
            let cli = Cli::parse_from(argv.clone());

            match (&cli.command, &expected) {
                (Some(parsed), Command::Relay(want)) => match parsed {
                    Command::Relay(args) => assert_eq!(args.bind, want.bind),
                    other => panic!("expected relay, got {other:?} for {argv:?}"),
                },
                (Some(parsed), Command::Worker(want)) => match parsed {
                    Command::Worker(args) => assert_eq!(args.admin_bind, want.admin_bind),
                    other => panic!("expected worker, got {other:?} for {argv:?}"),
                },
                (Some(parsed), Command::Openapi(OpenapiCommand::Export(want))) => match parsed {
                    Command::Openapi(OpenapiCommand::Export(args)) => {
                        assert_eq!(args.out, want.out)
                    }
                    other => panic!("expected openapi export, got {other:?} for {argv:?}"),
                },
                other => panic!("unexpected parse for {argv:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn relay_bind_argument_still_reaches_the_relay_command() {
        let cli = Cli::parse_from(["prompt-ferry", "relay", "--bind", "127.0.0.1:9000"]);

        match cli.command {
            Some(Command::Relay(args)) => {
                assert_eq!(args.bind.as_deref(), Some("127.0.0.1:9000"));
            }
            other => panic!("expected relay command, got {other:?}"),
        }
    }

    #[test]
    fn parse_worker_standalone_database_path() {
        let cli = Cli::parse_from([
            "prompt-ferry",
            "worker",
            "--standalone-database-path",
            "/var/lib/prompt-ferry/worker.sqlite3",
        ]);

        match cli.command {
            Some(Command::Worker(args)) => assert_eq!(
                args.standalone_database_path.as_deref(),
                Some("/var/lib/prompt-ferry/worker.sqlite3")
            ),
            other => panic!("expected worker command, got {other:?}"),
        }
    }
}
