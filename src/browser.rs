//! Opening the local admin UI in the default browser.
//!
//! Only the no-argument integrated startup on Windows launches a browser: the
//! user double-clicked an executable, so showing the UI is part of the job.
//! The console window is kept (logs stay visible, Ctrl+C and closing the
//! console stop the service), and a failure to launch — no association, no
//! default browser, a sandboxed environment — is logged and otherwise
//! ignored, because a server that cannot open a browser still serves.
//!
//! The decision logic is separated from the platform launch so the whole
//! surface is testable on every OS: the launch side is a trait object the
//! caller can replace with a recording stub.

use std::net::SocketAddr;

use crate::config::binds::local_ui_url;

/// Everything the launch decision needs: the entrypoint kind and the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchContext {
    /// True only for the no-argument integrated entrypoint.
    pub no_subcommand: bool,
    /// True on Windows builds.
    pub windows: bool,
}

impl LaunchContext {
    /// The integrated no-argument startup on Windows, and nothing else.
    pub fn should_launch(&self) -> bool {
        self.no_subcommand && self.windows
    }
}

/// Opens a URL; the production implementation shells out to the OS.
pub trait BrowserLauncher: Send + Sync {
    fn launch(&self, url: &str);
}

/// Best-effort launch: never fails the caller, only logs.
///
/// Only a loopback URL may reach the OS. A wildcard admin bind is opened
/// through loopback, which is how a local browser reaches it; a concrete
/// non-loopback bind (an operator bound the UI to an interface address) is
/// skipped with a log instead, because opening a non-loopback URL would point
/// the browser away from the machine it is running on.
pub fn open_admin_ui(
    context: LaunchContext,
    admin_addr: SocketAddr,
    launcher: &dyn BrowserLauncher,
) {
    if !context.should_launch() {
        return;
    }
    if !admin_addr.ip().is_loopback() && !admin_addr.ip().is_unspecified() {
        tracing::warn!(
            %admin_addr,
            "the admin UI is bound to a non-loopback address; skipping the browser launch"
        );
        return;
    }
    let url = local_ui_url(admin_addr);
    launcher.launch(&url);
}

/// The production launcher. Only the loopback URL is ever handed over, and a
/// failure is logged, never propagated.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemBrowserLauncher;

impl BrowserLauncher for SystemBrowserLauncher {
    #[cfg(windows)]
    fn launch(&self, url: &str) {
        // `open::that` is not used because it pulls a wider dependency set
        // than this single call needs.
        match std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
        {
            Ok(_) => tracing::info!(%url, "opened the admin UI in the default browser"),
            Err(error) => tracing::warn!(
                %url,
                error = %error,
                "could not open the default browser; the admin UI stays available at the URL"
            ),
        }
    }

    #[cfg(not(windows))]
    fn launch(&self, _url: &str) {
        // The decision layer never calls this on a non-Windows build; if a
        // future caller does, staying silent keeps the no-launch contract.
    }
}

/// Records launches instead of opening a browser.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct RecordingLauncher {
    pub urls: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl RecordingLauncher {
    pub(crate) fn urls(&self) -> Vec<String> {
        self.urls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[cfg(test)]
impl BrowserLauncher for RecordingLauncher {
    fn launch(&self, url: &str) {
        self.urls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(url.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{LaunchContext, RecordingLauncher, SystemBrowserLauncher, open_admin_ui};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    #[test]
    fn only_the_windows_no_subcommand_entrypoint_launches() {
        let launcher = RecordingLauncher::default();
        let admin = addr(8789);

        open_admin_ui(
            LaunchContext {
                no_subcommand: true,
                windows: true,
            },
            admin,
            &launcher,
        );
        assert_eq!(launcher.urls(), vec!["http://127.0.0.1:8789/".to_string()]);

        // The `serve` alias and component subcommands never launch.
        for context in [
            LaunchContext {
                no_subcommand: false,
                windows: true,
            },
            LaunchContext {
                no_subcommand: false,
                windows: false,
            },
            LaunchContext {
                no_subcommand: true,
                windows: false,
            },
        ] {
            let launcher = RecordingLauncher::default();
            open_admin_ui(context, admin, &launcher);
            assert!(
                launcher.urls().is_empty(),
                "context {context:?} must not launch"
            );
        }
    }

    #[test]
    fn a_wildcard_admin_bind_is_opened_through_loopback() {
        let launcher = RecordingLauncher::default();

        open_admin_ui(
            LaunchContext {
                no_subcommand: true,
                windows: true,
            },
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8789),
            &launcher,
        );

        assert_eq!(
            launcher.urls(),
            vec!["http://127.0.0.1:8789/".to_string()],
            "only the loopback URL may be opened"
        );
    }

    #[test]
    fn a_concrete_non_loopback_admin_bind_is_never_opened() {
        let launcher = RecordingLauncher::default();

        // An operator bound the UI to an interface address; the browser must
        // not be pointed at a URL that does not name this machine.
        open_admin_ui(
            LaunchContext {
                no_subcommand: true,
                windows: true,
            },
            SocketAddr::new(IpAddr::from([192, 0, 2, 10]), 8789),
            &launcher,
        );

        assert!(
            launcher.urls().is_empty(),
            "a non-loopback bind must skip the launch, got {:?}",
            launcher.urls()
        );
    }

    #[test]
    fn an_ipv6_non_loopback_admin_bind_is_never_opened() {
        let launcher = RecordingLauncher::default();

        open_admin_ui(
            LaunchContext {
                no_subcommand: true,
                windows: true,
            },
            SocketAddr::new(IpAddr::from([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]), 8789),
            &launcher,
        );

        assert!(launcher.urls().is_empty());
    }

    #[test]
    fn an_ipv6_loopback_admin_bind_is_opened_through_its_own_loopback_form() {
        let launcher = RecordingLauncher::default();

        open_admin_ui(
            LaunchContext {
                no_subcommand: true,
                windows: true,
            },
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8789),
            &launcher,
        );

        assert_eq!(
            launcher.urls(),
            vec!["http://[::1]:8789/".to_string()],
            "an IPv6 loopback bind keeps its own loopback URL"
        );
    }

    #[test]
    fn the_system_launcher_is_a_launch_side_that_stays_silent_off_windows() {
        // Constructing it on every platform keeps the type part of the API
        // surface; the non-Windows build must not shell out.
        let _ = SystemBrowserLauncher;
    }
}
