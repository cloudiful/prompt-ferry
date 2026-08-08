use crate::config::{RelayConfig, TlsMode, WorkerConfig};
use anyhow::{Context, anyhow};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer},
    server::WebPkiClientVerifier,
};
use rustls_pki_types::pem::PemObject;
use std::{fs, io::Cursor, sync::Arc};

pub fn validate_relay_config(config: &RelayConfig) -> anyhow::Result<()> {
    match config.tls_mode {
        TlsMode::Off => Ok(()),
        TlsMode::Server => {
            require_path(
                &config.tls_cert,
                "relay tls_cert is required when tls_mode=server",
            )?;
            require_path(
                &config.tls_key,
                "relay tls_key is required when tls_mode=server",
            )
        }
        TlsMode::Mtls => {
            require_path(
                &config.tls_cert,
                "relay tls_cert is required when tls_mode=mtls",
            )?;
            require_path(
                &config.tls_key,
                "relay tls_key is required when tls_mode=mtls",
            )?;
            require_path(
                &config.tls_client_ca,
                "relay tls_client_ca is required when tls_mode=mtls",
            )
        }
    }
}

pub fn validate_relay_worker_config(config: &RelayConfig) -> anyhow::Result<()> {
    match config.worker_tls_mode {
        TlsMode::Off => Ok(()),
        TlsMode::Server => {
            require_path(
                &config.worker_tls_cert,
                "relay worker_tls_cert is required when worker_tls_mode=server",
            )?;
            require_path(
                &config.worker_tls_key,
                "relay worker_tls_key is required when worker_tls_mode=server",
            )
        }
        TlsMode::Mtls => {
            require_path(
                &config.worker_tls_cert,
                "relay worker_tls_cert is required when worker_tls_mode=mtls",
            )?;
            require_path(
                &config.worker_tls_key,
                "relay worker_tls_key is required when worker_tls_mode=mtls",
            )?;
            require_path(
                &config.worker_tls_client_ca,
                "relay worker_tls_client_ca is required when worker_tls_mode=mtls",
            )
        }
    }
}

pub fn validate_worker_config(config: &WorkerConfig) -> anyhow::Result<()> {
    for relay_url in &config.relay_urls {
        let tls_mode = worker_tls_mode(config, relay_url)?;
        match tls_mode {
            TlsMode::Off => {
                if relay_url.starts_with("wss://") {
                    return Err(anyhow!("worker relay URL must use ws:// when tls_mode=off"));
                }
            }
            TlsMode::Server => require_wss(config, relay_url)?,
            TlsMode::Mtls => {
                require_wss(config, relay_url)?;
                require_path(
                    &config.client_cert,
                    "worker client_cert is required when tls_mode=mtls",
                )?;
                require_path(
                    &config.client_key,
                    "worker client_key is required when tls_mode=mtls",
                )?;
            }
        }
    }
    Ok(())
}

pub fn server_config(config: &RelayConfig) -> anyhow::Result<Arc<ServerConfig>> {
    let certs = load_certs(&config.tls_cert)?;
    let key = load_key(&config.tls_key)?;
    let builder = ServerConfig::builder();
    let builder = match config.tls_mode {
        TlsMode::Off => return Err(anyhow!("tls server config requested when tls_mode=off")),
        TlsMode::Server => builder.with_no_client_auth(),
        TlsMode::Mtls => {
            let roots = load_root_store(&config.tls_client_ca)?;
            let verifier = WebPkiClientVerifier::builder(roots.into())
                .build()
                .context("failed to build client certificate verifier")?;
            builder.with_client_cert_verifier(verifier)
        }
    };
    Ok(Arc::new(
        builder
            .with_single_cert(certs, key)
            .context("failed to build relay TLS server config")?,
    ))
}

pub fn worker_server_config(config: &RelayConfig) -> anyhow::Result<Arc<ServerConfig>> {
    let certs = load_certs(&config.worker_tls_cert)?;
    let key = load_key(&config.worker_tls_key)?;
    let builder = ServerConfig::builder();
    let builder = match config.worker_tls_mode {
        TlsMode::Off => {
            return Err(anyhow!(
                "tls server config requested when worker_tls_mode=off"
            ));
        }
        TlsMode::Server => builder.with_no_client_auth(),
        TlsMode::Mtls => {
            let roots = load_root_store(&config.worker_tls_client_ca)?;
            let verifier = WebPkiClientVerifier::builder(roots.into())
                .build()
                .context("failed to build worker client certificate verifier")?;
            builder.with_client_cert_verifier(verifier)
        }
    };
    Ok(Arc::new(builder.with_single_cert(certs, key).context(
        "failed to build relay worker TLS server config",
    )?))
}

pub fn validate_worker_relay_material(
    relay_url: &str,
    tls_mode: TlsMode,
    relay_ca_pem: Option<&str>,
    client_cert_pem: Option<&str>,
    client_key_pem: Option<&str>,
) -> anyhow::Result<()> {
    match tls_mode {
        TlsMode::Off => {
            if relay_url.starts_with("wss://") {
                return Err(anyhow!("worker relay URL must use ws:// when tls_mode=off"));
            }
        }
        TlsMode::Server => {
            require_wss_url(relay_url, tls_mode)?;
            if let Some(relay_ca_pem) = relay_ca_pem.filter(|value| !value.trim().is_empty()) {
                load_root_store_pem(relay_ca_pem)?;
            }
        }
        TlsMode::Mtls => {
            require_wss_url(relay_url, tls_mode)?;
            let client_cert_pem = client_cert_pem
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("worker client_cert is required when tls_mode=mtls"))?;
            let client_key_pem = client_key_pem
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow!("worker client_key is required when tls_mode=mtls"))?;
            load_certs_pem(client_cert_pem)?;
            load_key_pem(client_key_pem)?;
            if let Some(relay_ca_pem) = relay_ca_pem.filter(|value| !value.trim().is_empty()) {
                load_root_store_pem(relay_ca_pem)?;
            }
        }
    }
    Ok(())
}

pub fn client_config_from_pem(
    relay_url: &str,
    tls_mode: TlsMode,
    relay_ca_pem: Option<&str>,
    client_cert_pem: Option<&str>,
    client_key_pem: Option<&str>,
) -> anyhow::Result<Arc<ClientConfig>> {
    let roots = worker_root_store_pem(relay_ca_pem)?;
    let builder = ClientConfig::builder().with_root_certificates(roots);
    let client =
        match tls_mode {
            TlsMode::Off => return Err(anyhow!("tls client config requested when tls_mode=off")),
            TlsMode::Server => builder.with_no_client_auth(),
            TlsMode::Mtls => builder
                .with_client_auth_cert(
                    load_certs_pem(client_cert_pem.ok_or_else(|| {
                        anyhow!("worker client_cert is required when tls_mode=mtls")
                    })?)?,
                    load_key_pem(client_key_pem.ok_or_else(|| {
                        anyhow!("worker client_key is required when tls_mode=mtls")
                    })?)?,
                )
                .context("failed to build worker mTLS client config")?,
        };
    require_wss_url(relay_url, tls_mode)?;
    Ok(Arc::new(client))
}

pub fn read_pem_file(path: &str) -> anyhow::Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read PEM file {path}"))
}

pub fn worker_tls_mode(config: &WorkerConfig, relay_url: &str) -> anyhow::Result<TlsMode> {
    if let Some(mode) = config.tls_mode.explicit() {
        return Ok(mode);
    }
    if relay_url.starts_with("wss://") {
        return Ok(TlsMode::Server);
    }
    if relay_url.starts_with("ws://") {
        return Ok(TlsMode::Off);
    }
    Err(anyhow!("worker relay URL must use ws:// or wss://"))
}

fn worker_root_store_pem(relay_ca_pem: Option<&str>) -> anyhow::Result<RootCertStore> {
    match relay_ca_pem.filter(|value| !value.trim().is_empty()) {
        Some(relay_ca_pem) => load_root_store_pem(relay_ca_pem),
        None => load_native_root_store(),
    }
}

fn require_path(value: &str, message: &'static str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        Err(anyhow!(message))
    } else {
        Ok(())
    }
}

fn require_wss(config: &WorkerConfig, relay_url: &str) -> anyhow::Result<()> {
    if !relay_url.starts_with("wss://") {
        return Err(anyhow!(
            "worker relay URL must use wss:// when tls_mode={:?}",
            worker_tls_mode(config, relay_url).unwrap_or(TlsMode::Off)
        ));
    }
    Ok(())
}

fn require_wss_url(relay_url: &str, tls_mode: TlsMode) -> anyhow::Result<()> {
    if !relay_url.starts_with("wss://") {
        return Err(anyhow!(
            "worker relay URL must use wss:// when tls_mode={}",
            tls_mode.as_str()
        ));
    }
    Ok(())
}

fn load_certs(path: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let pem = read_pem_file(path)?;
    load_certs_pem(&pem).with_context(|| format!("failed to parse certificate PEM {path}"))
}

fn load_certs_pem(pem: &str) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let reader = std::io::BufReader::new(Cursor::new(pem.as_bytes()));
    let certs = CertificateDer::pem_reader_iter(reader)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse certificate PEM")?;
    if certs.is_empty() {
        return Err(anyhow!("no certificates found in PEM"));
    }
    Ok(certs)
}

fn load_key(path: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    let pem = read_pem_file(path)?;
    load_key_pem(&pem).with_context(|| format!("failed to parse private key PEM {path}"))
}

fn load_key_pem(pem: &str) -> anyhow::Result<PrivateKeyDer<'static>> {
    PrivateKeyDer::from_pem_slice(pem.as_bytes()).context("failed to parse private key PEM")
}

fn load_root_store(path: &str) -> anyhow::Result<RootCertStore> {
    let pem = read_pem_file(path)?;
    load_root_store_pem(&pem).with_context(|| format!("no valid CA certificates found in {path}"))
}

fn load_root_store_pem(pem: &str) -> anyhow::Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    let certs = load_certs_pem(pem)?;
    let (added, ignored) = roots.add_parsable_certificates(certs);
    if added == 0 {
        return Err(anyhow!("no valid CA certificates found in PEM"));
    }
    if ignored > 0 {
        tracing::warn!(ignored, "ignored invalid CA certificates");
    }
    Ok(roots)
}

fn load_native_root_store() -> anyhow::Result<RootCertStore> {
    let native_certs = rustls_native_certs::load_native_certs();
    let mut roots = RootCertStore::empty();
    let (added, ignored) = roots.add_parsable_certificates(native_certs.certs);
    for err in native_certs.errors {
        tracing::warn!(error = %err, "failed to load native CA certificate");
    }
    if added == 0 {
        return Err(anyhow!("no valid native CA certificates found"));
    }
    if ignored > 0 {
        tracing::warn!(ignored, "ignored invalid native CA certificates");
    }
    Ok(roots)
}
