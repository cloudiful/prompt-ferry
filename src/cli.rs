use crate::config::{BridgeEncryptionMode, NativeApi, TlsMode, WorkerTlsMode};
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "prompt-ferry",
    version,
    about = "OpenAI-compatible relay and worker for upstream ferrying"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
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
    pub endpoint_model_cache_ttl_seconds: Option<u64>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct ServeArgs {
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
    fn parse_serve_command_with_internal_worker_bind() {
        let cli = Cli::parse_from([
            "prompt-ferry",
            "serve",
            "--internal-worker-bind",
            "127.0.0.1:9788",
        ]);

        match cli.command {
            Command::Serve(args) => {
                assert_eq!(args.internal_worker_bind.as_deref(), Some("127.0.0.1:9788"));
            }
            other => panic!("expected serve command, got {other:?}"),
        }
    }
}
