use crate::{
    cli::ServeArgs,
    config::{AppConfig, BridgeEncryptionMode, RelayConfig, TlsMode, WorkerConfig, WorkerTlsMode},
    relay, worker,
};
use anyhow::{Context, anyhow};
use tracing::info;

pub async fn run(app_config: AppConfig, args: ServeArgs) -> anyhow::Result<()> {
    let (relay_config, worker_config) = derive_configs(app_config, args)?;

    info!(
        public_bind = %relay_config.bind,
        internal_worker_bind = %relay_config.worker_bind,
        admin_bind = %worker_config.admin_bind,
        "serve mode starting"
    );

    tokio::try_join!(
        relay::run_embedded(relay_config),
        worker::run_embedded(worker_config),
    )?;
    Ok(())
}

fn derive_configs(
    app_config: AppConfig,
    args: ServeArgs,
) -> anyhow::Result<(RelayConfig, WorkerConfig)> {
    let serve_config = app_config.serve.merge_args(args);
    let mut relay_config = app_config.relay;
    let mut worker_config = app_config.worker;

    validate_loopback_bind(&serve_config.internal_worker_bind)?;
    relay_config.worker_bind = serve_config.internal_worker_bind.clone();
    worker_config.relay_urls = vec![format!("ws://{}/ws/worker", relay_config.worker_bind)];

    // The worker-side token is authoritative in serve mode: an explicitly
    // empty worker token fully opens serve-mode worker auth even when the
    // relay config carries its default or a custom token.
    let worker_token = worker_config.worker_token.trim().to_string();
    relay_config.worker_token = worker_token.clone();
    worker_config.worker_token = worker_token;

    relay_config.worker_tls_mode = TlsMode::Off;
    relay_config.worker_tls_cert.clear();
    relay_config.worker_tls_key.clear();
    relay_config.worker_tls_client_ca.clear();

    worker_config.tls_mode = WorkerTlsMode::Off;
    worker_config.relay_ca.clear();
    worker_config.client_cert.clear();
    worker_config.client_key.clear();

    relay_config.bridge_encryption_mode = BridgeEncryptionMode::Off;
    relay_config.bridge_encryption_key.clear();
    worker_config.bridge_encryption_mode = BridgeEncryptionMode::Off;
    worker_config.bridge_encryption_key.clear();

    Ok((relay_config, worker_config))
}

fn validate_loopback_bind(bind: &str) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = bind
        .parse()
        .with_context(|| format!("invalid internal worker bind address `{bind}`"))?;
    if !addr.ip().is_loopback() {
        return Err(anyhow!(
            "serve internal worker bind must use a loopback address"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn derive_configs_for_serve_mode_overrides_internal_bridge_settings() {
        let mut app_config = AppConfig::default();
        app_config.relay.worker_token = "relay-token".to_string();
        app_config.worker.worker_token = "worker-token".to_string();
        app_config.worker.tls_mode = WorkerTlsMode::Mtls;
        app_config.worker.relay_ca = "/tmp/relay-ca.pem".to_string();
        app_config.worker.client_cert = "/tmp/client.crt".to_string();
        app_config.worker.client_key = "/tmp/client.key".to_string();
        app_config.worker.bridge_encryption_mode = BridgeEncryptionMode::Required;
        app_config.worker.bridge_encryption_key = "worker-key".to_string();
        app_config.relay.worker_tls_mode = TlsMode::Mtls;
        app_config.relay.worker_tls_cert = "/tmp/worker.crt".to_string();
        app_config.relay.worker_tls_key = "/tmp/worker.key".to_string();
        app_config.relay.worker_tls_client_ca = "/tmp/worker-ca.pem".to_string();
        app_config.relay.bridge_encryption_mode = BridgeEncryptionMode::Required;
        app_config.relay.bridge_encryption_key = "relay-key".to_string();

        let (relay_config, worker_config) =
            derive_configs(app_config, ServeArgs::default()).unwrap();

        assert_eq!(relay_config.worker_bind, "127.0.0.1:8788");
        assert_eq!(
            worker_config.relay_urls,
            vec!["ws://127.0.0.1:8788/ws/worker".to_string()]
        );
        assert_eq!(relay_config.worker_token, "worker-token");
        assert_eq!(worker_config.worker_token, "worker-token");
        assert_eq!(relay_config.worker_tls_mode, TlsMode::Off);
        assert!(relay_config.worker_tls_cert.is_empty());
        assert!(relay_config.worker_tls_key.is_empty());
        assert!(relay_config.worker_tls_client_ca.is_empty());
        assert_eq!(worker_config.tls_mode, WorkerTlsMode::Off);
        assert!(worker_config.relay_ca.is_empty());
        assert!(worker_config.client_cert.is_empty());
        assert!(worker_config.client_key.is_empty());
        assert_eq!(
            relay_config.bridge_encryption_mode,
            BridgeEncryptionMode::Off
        );
        assert!(relay_config.bridge_encryption_key.is_empty());
        assert_eq!(
            worker_config.bridge_encryption_mode,
            BridgeEncryptionMode::Off
        );
        assert!(worker_config.bridge_encryption_key.is_empty());
    }

    #[test]
    fn serve_mode_requires_loopback_internal_worker_bind() {
        let err = derive_configs(
            AppConfig::default(),
            ServeArgs {
                internal_worker_bind: Some("0.0.0.0:8788".to_string()),
            },
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("serve internal worker bind must use a loopback address")
        );
    }

    #[test]
    fn serve_mode_preserves_empty_worker_token_on_both_sides() {
        let mut app_config = AppConfig::default();
        app_config.relay.worker_token = String::new();
        app_config.worker.worker_token = String::new();

        let (relay_config, worker_config) =
            derive_configs(app_config, ServeArgs::default()).unwrap();

        assert_eq!(relay_config.worker_token, "");
        assert_eq!(worker_config.worker_token, "");
    }

    #[test]
    fn serve_mode_empty_worker_token_overrides_nonempty_relay_token() {
        let mut app_config = AppConfig::default();
        app_config.relay.worker_token = "relay-default-token".to_string();
        app_config.worker.worker_token = String::new();

        let (relay_config, worker_config) =
            derive_configs(app_config, ServeArgs::default()).unwrap();

        assert_eq!(relay_config.worker_token, "");
        assert_eq!(worker_config.worker_token, "");
    }
}
