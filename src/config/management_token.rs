//! The relay management token this host serves with.
//!
//! The relay's management listener is loopback-only, which keeps it off the
//! network but says nothing about which local caller is the operator. This
//! resolves the token that decides that, so a host with no configured token
//! still runs a control API nobody can drive by accident.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng as _;
use tracing::warn;

use crate::{
    config::{RelayConfig, host_store},
    runtime_env,
};

/// Resolve the management token, generating and persisting one when unset.
///
/// A configured token is used as written. Otherwise the token already in the
/// host-local configuration is reused, and failing that a new one is generated
/// and stored before it is used. Failing to store it stops the start instead of
/// falling back to an open control API.
pub fn resolve(relay_config: &mut RelayConfig) -> anyhow::Result<()> {
    if !relay_config.admin_token.trim().is_empty() {
        return Ok(());
    }
    let token = match stored_token() {
        Some(token) => token,
        None => {
            let generated = generate();
            host_store::save_admin_token(&generated).map_err(|error| {
                anyhow::anyhow!(
                    "failed to store the generated relay management token in the host-local \
                     configuration: {error}"
                )
            })?;
            generated
        }
    };
    warn!(
        "the relay management API uses the host-local management token from {}; keep that file \
         owner-only or set `relay.admin_token` yourself",
        host_config_location()
    );
    relay_config.admin_token = token;
    Ok(())
}

fn stored_token() -> Option<String> {
    host_store::load()
        .ok()
        .and_then(|overlay| overlay.relay.admin_token)
        .filter(|token| !token.trim().is_empty())
}

fn host_config_location() -> String {
    runtime_env::host_config_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|error| format!("the host-local configuration ({error})"))
}

fn generate() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::{generate, resolve};
    use crate::config::RelayConfig;

    #[test]
    fn generated_tokens_are_long_and_distinct() {
        let first = generate();
        let second = generate();

        // 32 random bytes, URL-safe and unpadded: a token a person cannot guess
        // and does not have to copy by hand out of a log line.
        assert_eq!(first.len(), 43, "unexpected token: {first}");
        assert!(!first.contains('='), "unexpected token: {first}");
        assert_ne!(first, second);
    }

    #[test]
    fn a_configured_token_is_used_as_written() {
        let mut relay_config = RelayConfig {
            admin_token: "  operator-chosen-token ".to_string(),
            ..RelayConfig::default()
        };

        resolve(&mut relay_config).expect("a configured token needs no write");

        assert_eq!(relay_config.admin_token, "  operator-chosen-token ");
    }
}
