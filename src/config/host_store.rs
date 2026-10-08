//! The writable, host-local settings the relay control plane owns.
//!
//! The relay's management page can change two things that belong to this
//! machine alone: which services it starts, and the token that guards its own
//! management API. Both live in a small overlay next to the main
//! configuration rather than in the main file or in the shared configuration a
//! managed relay list can carry between hosts.
//!
//! Every field is optional, and an absent field simply is not part of the
//! overlay: a file that only carries a role leaves the management bind and token
//! where the operator configured them, and an empty overlay changes nothing. The
//! overlay therefore wins over the main configuration for the keys it defines
//! and is silent about the rest.
//!
//! Saving rewrites only this file. The main configuration an operator maintains
//! by hand is never rewritten, so a save can neither drop an unknown key nor
//! materialise a value that only ever existed in the environment.

use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, HostRole};
use crate::runtime_env;

/// The host-local overlay, as stored on disk.
///
/// Field names mirror the configuration sections they overlay so a saved file
/// reads as the settings it changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostOverlay {
    pub host: HostOverlayRole,
    pub relay: HostOverlayRelay,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostOverlayRole {
    /// Which services this host runs; absent means "follow the main config".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<HostRole>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostOverlayRelay {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_bind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_token: Option<String>,
}

impl HostOverlay {
    /// Whether this overlay defines nothing at all.
    pub fn is_empty(&self) -> bool {
        self.host.role.is_none()
            && self.relay.admin_bind.is_none()
            && self.relay.admin_token.is_none()
    }
}

/// Overlay this process was started from, shared with the control plane.
///
/// The startup path resolves the overlay once and installs it, so the running
/// relay answers with the role it actually started as instead of re-reading the
/// file that a save is about to rewrite.
static ACTIVE: std::sync::OnceLock<std::sync::RwLock<HostOverlay>> = std::sync::OnceLock::new();

fn active() -> &'static std::sync::RwLock<HostOverlay> {
    ACTIVE.get_or_init(|| std::sync::RwLock::new(HostOverlay::default()))
}

/// The recorded host-local overlay this process started from, shared with the
/// control plane.
pub fn active_overlay() -> HostOverlay {
    active()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// The role the running process started as, as far as the overlay records it.
///
/// `None` means the main configuration decided, which is what every
/// installation without a saved role reports.
pub fn active_role() -> Option<HostRole> {
    active_overlay().host.role
}

/// Record the overlay this process started from.
pub fn activate(overlay: HostOverlay) {
    *active()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = overlay;
}

/// Forget the recorded overlay. Tests use this to reach a clean startup again.
#[cfg(test)]
pub fn clear() {
    activate(HostOverlay::default());
}

/// Narrow the host-local file to its owner. It carries a management token.
fn restrict_to_owner() -> std::io::Result<()> {
    let path = runtime_env::host_config_path()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    runtime_env::restrict_to_owner(&path).map_err(|error| std::io::Error::other(error.to_string()))
}

/// Read the overlay from disk.
///
/// A missing file is not an error: the main configuration stays in charge and
/// the file is created when the first value is actually saved.
pub fn load() -> std::io::Result<HostOverlay> {
    let overlay: HostOverlay = crate::config::read(
        runtime_env::host_config_app_name(),
        Some(crate::config::ReadOptions::default().without_dotenv()),
    )?;
    restrict_to_owner()?;
    Ok(overlay)
}

/// Write the overlay back after applying `change`.
///
/// The read is deliberate: the file is the process's own store, and preserving
/// the keys it already carries is what keeps a role save from discarding a
/// management token saved earlier.
pub fn update(change: impl FnOnce(&mut HostOverlay)) -> std::io::Result<HostOverlay> {
    let mut overlay = load()?;
    change(&mut overlay);
    crate::config::save(runtime_env::host_config_app_name(), &overlay)?;
    restrict_to_owner()?;
    activate(overlay.clone());
    Ok(overlay)
}

/// Save the role this host should start as.
pub fn save_role(role: HostRole) -> std::io::Result<()> {
    update(|overlay| overlay.host.role = Some(role)).map(|_| ())
}

/// Save the token guarding the relay management API.
pub fn save_admin_token(token: &str) -> std::io::Result<()> {
    update(|overlay| overlay.relay.admin_token = Some(token.to_string())).map(|_| ())
}

/// Apply the overlay to a freshly read configuration.
///
/// A failure is reported and the main configuration is kept: startup must not
/// depend on a file that may not exist yet or may be read-only, and the control
/// plane reports the real failure when a save is actually attempted.
pub fn apply(app_config: &mut AppConfig) {
    let overlay = match load() {
        Ok(overlay) => overlay,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "host-local configuration could not be read; the main configuration is in charge"
            );
            return;
        }
    };
    if let Some(role) = overlay.host.role {
        app_config.host.role = role;
    }
    if let Some(bind) = overlay.relay.admin_bind.clone() {
        app_config.relay.admin_bind = bind;
    }
    if let Some(token) = overlay.relay.admin_token.clone() {
        app_config.relay.admin_token = token;
    }
    activate(overlay);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_overlay_defines_nothing() {
        assert!(HostOverlay::default().is_empty());
        assert!(
            !HostOverlay {
                host: HostOverlayRole {
                    role: Some(HostRole::Relay)
                },
                relay: HostOverlayRelay::default(),
            }
            .is_empty()
        );
    }

    #[test]
    fn an_overlay_serializes_only_the_keys_it_defines() {
        // A file that only carries a role must not mention the other keys at
        // all: an empty string or a default bind written here would silently
        // override what the operator configured.
        let json = serde_json::to_string(&HostOverlay {
            host: HostOverlayRole {
                role: Some(HostRole::Relay),
            },
            relay: HostOverlayRelay::default(),
        })
        .expect("serialize the overlay");

        assert_eq!(
            json, r#"{"host":{"role":"relay"},"relay":{}}"#,
            "an undefined key must be absent from the file, not written as null"
        );
        assert_eq!(
            serde_json::from_str::<HostOverlay>(&json).expect("parse the overlay"),
            HostOverlay {
                host: HostOverlayRole {
                    role: Some(HostRole::Relay)
                },
                relay: HostOverlayRelay::default(),
            }
        );
    }

    #[test]
    fn an_empty_file_leaves_the_main_configuration_in_charge() {
        // This is the upgrade path: an installation that never saved a role has
        // no overlay, or an empty one, and keeps the role its main config names.
        let overlay = serde_json::from_str::<HostOverlay>("{}").expect("parse an empty overlay");
        assert!(overlay.is_empty());

        let mut app_config = AppConfig::default();
        app_config.host.role = HostRole::Worker;
        overlay
            .host
            .role
            .inspect(|role| app_config.host.role = *role);
        assert_eq!(app_config.host.role, HostRole::Worker);
    }

    #[test]
    fn a_saved_role_wins_over_the_main_configuration() {
        let overlay = HostOverlay {
            host: HostOverlayRole {
                role: Some(HostRole::Relay),
            },
            relay: HostOverlayRelay::default(),
        };

        let mut app_config = AppConfig::default();
        assert_eq!(app_config.host.role, HostRole::Integrated);
        if let Some(role) = overlay.host.role {
            app_config.host.role = role;
        }

        assert_eq!(app_config.host.role, HostRole::Relay);
    }

    #[test]
    fn the_activated_overlay_reports_the_role_the_process_started_as() {
        clear();
        assert_eq!(active_role(), None);

        activate(HostOverlay {
            host: HostOverlayRole {
                role: Some(HostRole::Worker),
            },
            relay: HostOverlayRelay::default(),
        });
        assert_eq!(active_role(), Some(HostRole::Worker));

        // Recording again replaces the previous role, so a restart inside one
        // process never answers with the role it stopped running.
        activate(HostOverlay {
            host: HostOverlayRole {
                role: Some(HostRole::Relay),
            },
            relay: HostOverlayRelay::default(),
        });
        assert_eq!(active_role(), Some(HostRole::Relay));
        clear();
    }

    #[test]
    fn an_unrecognized_role_is_rejected_rather_than_defaulted() {
        let error = serde_json::from_str::<HostOverlay>(r#"{"host":{"role":"worker-only"}}"#)
            .expect_err("an overlay must not start a host in the wrong mode");
        assert!(error.to_string().contains("worker-only"), "{error}");
    }
}
