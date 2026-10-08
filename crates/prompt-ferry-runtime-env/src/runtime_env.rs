use crate::naming::{CONFIG_APP_NAME, CONFIG_ENV_PREFIX};
use anyhow::{Result, anyhow};
pub use db_init::DatabaseUrlResolution;
use db_init::{load_dotenv_if_exists, resolve_database_url as resolve_shared_database_url};
use std::{
    env,
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

/// Canonical environment variable carrying the PostgreSQL URL. The worker
/// config file field `worker.database_url` and an explicit `--database-url`
/// CLI argument remain supported; no other environment alias is read.
const DATABASE_URL_ENV_KEYS: &[&str] = &["DATABASE_URL"];

/// Config application that holds the writable host-local overlay.
///
/// It is a separate application directory from [`CONFIG_APP_NAME`] so the
/// relay control plane can write the keys it owns without ever rewriting — or
/// reading a secret out of — the operator's main `config.toml`.
const HOST_CONFIG_APP_NAME: &str = "prompt-ferry-host";

pub fn load_dotenv(path: impl AsRef<Path>) -> Result<()> {
    load_dotenv_if_exists(path)
}

pub fn resolve_database_url(from_arg: Option<String>) -> Result<DatabaseUrlResolution> {
    resolve_shared_database_url(from_arg, DATABASE_URL_ENV_KEYS, || {
        let config = config::read::<serde_json::Value>(
            CONFIG_APP_NAME,
            Some(config::ReadOptions::with_env_prefix(CONFIG_ENV_PREFIX)),
        )
        .map_err(|error| anyhow!(error).context("failed to load prompt-ferry config"))?;
        let url = config
            .get("worker")
            .and_then(|value| value.get("database_url"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Ok(url)
    })
}

pub fn select_config_app_name() -> Result<&'static str> {
    let _ = default_config_path(CONFIG_APP_NAME)?;
    Ok(CONFIG_APP_NAME)
}

pub fn resolve_standalone_database_path(configured_path: &str) -> Result<PathBuf> {
    resolve_standalone_database_path_from(configured_path, |key| env::var_os(key))
}

/// Resolve the local raw-payload object directory. An explicit configuration
/// value always wins; otherwise the deterministic default lives under the
/// platform data root next to the standalone database.
pub fn resolve_raw_object_store_local_dir(configured_dir: &str) -> Result<PathBuf> {
    resolve_raw_object_store_local_dir_from(configured_dir, |key| env::var_os(key))
}

/// Resolve a named runtime data file (encryption key, bootstrap admin
/// password) under `<data-root>/<app-name>/`.
pub fn prompt_ferry_data_file(file_name: &str) -> Result<PathBuf> {
    Ok(data_root_from(|key| env::var_os(key))?
        .join(CONFIG_APP_NAME)
        .join(file_name))
}

/// Create `path` exclusively with owner-only permissions on Unix, creating
/// parent directories as needed. Returns `false` without modifying anything
/// when the file already exists.
pub fn create_private_file_exclusive(path: &Path, contents: &str) -> Result<bool> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            anyhow!(error).context(format!(
                "failed to create parent directory {}",
                parent.display()
            ))
        })?;
    }
    match private_file_options().create_new(true).open(path) {
        Ok(mut file) => {
            use std::io::Write as _;
            file.write_all(contents.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| {
                    anyhow!(error).context(format!("failed to write {}", path.display()))
                })?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(anyhow!(error).context(format!("failed to create {}", path.display()))),
    }
}

/// Application name of the writable host-local configuration.
///
/// Exposed next to [`host_config_path`] so a caller that reads or writes it
/// through the config crate names the same application the path resolves to.
pub fn host_config_app_name() -> &'static str {
    HOST_CONFIG_APP_NAME
}

/// Path of the writable host-local configuration file.
///
/// It sits beside the main configuration in its own application directory and
/// holds only what the relay control plane owns — the host service role and the
/// local management token — so a save never touches the shared configuration.
pub fn host_config_path() -> Result<PathBuf> {
    default_config_path(HOST_CONFIG_APP_NAME)
}

/// Restrict an existing file to its owner.
///
/// The host-local configuration carries a management token, and the config
/// crate writes through a temporary file with default permissions; this narrows
/// the result afterwards. A no-op where Unix modes do not exist.
pub fn restrict_to_owner(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                anyhow!(error).context(format!(
                    "failed to restrict permissions of {}",
                    path.display()
                ))
            },
        )?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn private_file_options() -> std::fs::OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).mode(0o600);
        options
    }
    #[cfg(not(unix))]
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true);
        options
    }
}

fn resolve_raw_object_store_local_dir_from(
    configured_dir: &str,
    get_env: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    let configured_dir = configured_dir.trim();
    if !configured_dir.is_empty() {
        return Ok(PathBuf::from(configured_dir));
    }
    Ok(data_root_from(get_env)?
        .join(CONFIG_APP_NAME)
        .join("raw-objects"))
}

fn resolve_standalone_database_path_from(
    configured_path: &str,
    get_env: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    let configured_path = configured_path.trim();
    if !configured_path.is_empty() {
        return Ok(PathBuf::from(configured_path));
    }

    Ok(data_root_from(get_env)?
        .join(CONFIG_APP_NAME)
        .join("worker.sqlite3"))
}

fn default_config_path(app_name: &str) -> Result<PathBuf> {
    let app_name = Path::new(app_name);
    validate_app_name(app_name)?;
    Ok(config_root_from(|key| env::var_os(key))?
        .join(app_name)
        .join("config.toml"))
}

fn data_root_from(get_env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    data_root_from_platform(get_env)
}

fn validate_app_name(app_name: &Path) -> Result<()> {
    if app_name.as_os_str().is_empty() {
        return Err(anyhow!("app name must not be empty"));
    }
    if app_name.is_absolute() {
        return Err(anyhow!(
            "app name must be relative, got {}",
            app_name.display()
        ));
    }
    match app_name.components().next() {
        Some(Component::Normal(_)) if app_name.components().count() == 1 => Ok(()),
        _ => Err(anyhow!(
            "app name must be a single path component, got {}",
            app_name.display()
        )),
    }
}

fn env_path_with(get_env: impl Fn(&str) -> Option<OsString>, key: &str) -> Option<PathBuf> {
    get_env(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(windows)]
fn config_root_from(get_env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    env_path_with(&get_env, "APPDATA")
        .or_else(|| {
            env_path_with(&get_env, "USERPROFILE").map(|path| path.join("AppData").join("Roaming"))
        })
        .or_else(|| match (get_env("HOMEDRIVE"), get_env("HOMEPATH")) {
            (Some(drive), Some(path)) if !drive.is_empty() && !path.is_empty() => {
                Some(PathBuf::from(drive).join(path))
            }
            _ => None,
        })
        .map(|path| {
            if path.ends_with("Roaming") {
                path
            } else {
                path.join("AppData").join("Roaming")
            }
        })
        .ok_or_else(|| anyhow!("failed to resolve Windows config directory"))
}

#[cfg(windows)]
fn data_root_from_platform(get_env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    env_path_with(&get_env, "LOCALAPPDATA")
        .or_else(|| env_path_with(&get_env, "APPDATA"))
        .or_else(|| {
            env_path_with(&get_env, "USERPROFILE").map(|path| path.join("AppData").join("Local"))
        })
        .or_else(|| match (get_env("HOMEDRIVE"), get_env("HOMEPATH")) {
            (Some(drive), Some(path)) if !drive.is_empty() && !path.is_empty() => Some(
                PathBuf::from(drive)
                    .join(path)
                    .join("AppData")
                    .join("Local"),
            ),
            _ => None,
        })
        .ok_or_else(|| anyhow!("failed to resolve Windows data directory"))
}

#[cfg(target_os = "macos")]
fn data_root_from_platform(get_env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    env_path_with(get_env, "HOME")
        .map(|path| path.join("Library").join("Application Support"))
        .ok_or_else(|| anyhow!("failed to resolve macOS data directory from HOME"))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn data_root_from_platform(get_env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    env_path_with(&get_env, "XDG_DATA_HOME")
        .or_else(|| env_path_with(get_env, "HOME").map(|path| path.join(".local").join("share")))
        .ok_or_else(|| anyhow!("failed to resolve data directory from XDG_DATA_HOME or HOME"))
}

#[cfg(target_os = "macos")]
fn config_root_from(get_env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    env_path_with(get_env, "HOME")
        .map(|path| path.join("Library").join("Application Support"))
        .ok_or_else(|| anyhow!("failed to resolve macOS config directory from HOME"))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn config_root_from(get_env: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    env_path_with(&get_env, "XDG_CONFIG_HOME")
        .or_else(|| env_path_with(get_env, "HOME").map(|path| path.join(".config")))
        .ok_or_else(|| anyhow!("failed to resolve config directory from XDG_CONFIG_HOME or HOME"))
}

#[cfg(test)]
mod tests {
    use super::{
        create_private_file_exclusive, host_config_path, resolve_database_url,
        resolve_standalone_database_path_from, restrict_to_owner,
    };
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    #[test]
    fn the_host_local_config_sits_beside_the_main_config_in_its_own_directory() {
        let host = host_config_path().expect("resolve the host-local config path");
        let main = super::default_config_path(super::CONFIG_APP_NAME).expect("main config path");

        assert_ne!(
            host.parent().and_then(|dir| dir.file_name()),
            main.parent().and_then(|dir| dir.file_name()),
            "the writable overlay must not be the main configuration"
        );
        assert_eq!(
            host.parent()
                .and_then(Path::parent)
                .and_then(|dir| dir.file_name()),
            main.parent()
                .and_then(Path::parent)
                .and_then(|dir| dir.file_name()),
            "both live under the platform config root"
        );
        assert_ne!(
            host.parent().and_then(|dir| dir.file_name()),
            main.parent().and_then(|dir| dir.file_name()),
            "the writable overlay must not be the main configuration"
        );
    }

    #[cfg(unix)]
    #[test]
    fn restricting_a_file_leaves_it_readable_only_by_its_owner() {
        let dir =
            std::env::temp_dir().join(format!("prompt-ferry-restrict-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("host.toml");
        std::fs::write(&path, "role = \"relay\"").unwrap();

        restrict_to_owner(&path).unwrap();

        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "role = \"relay\"");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn private_file_create_is_exclusive_and_keeps_contents() {
        let dir = std::env::temp_dir().join(format!(
            "prompt-ferry-private-file-{}",
            uuid::Uuid::new_v4()
        ));
        let path = dir.join("nested").join("secret.key");

        assert!(create_private_file_exclusive(&path, "first").unwrap());
        assert!(!create_private_file_exclusive(&path, "second").unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn database_url_resolution_prefers_arg_then_database_url_env() {
        unsafe {
            std::env::remove_var("DATABASE_URL");
        }

        assert_eq!(
            resolve_database_url(Some(" postgres://arg ".to_string()))
                .unwrap()
                .database_url,
            "postgres://arg"
        );

        unsafe { std::env::set_var("DATABASE_URL", "postgres://default") };
        assert_eq!(
            resolve_database_url(None).unwrap().database_url,
            "postgres://default"
        );

        unsafe { std::env::remove_var("DATABASE_URL") };
    }

    #[test]
    fn standalone_database_path_trims_explicit_override() {
        assert_eq!(
            resolve_standalone_database_path_from("  ./state/worker.sqlite3  ", |_| None).unwrap(),
            PathBuf::from("./state/worker.sqlite3")
        );
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    #[test]
    fn standalone_database_path_uses_xdg_data_home_by_default() {
        assert_eq!(
            resolve_standalone_database_path_from("", |key| {
                (key == "XDG_DATA_HOME").then(|| OsString::from("/tmp/prompt-ferry-data"))
            })
            .unwrap(),
            PathBuf::from("/tmp/prompt-ferry-data/prompt-ferry/worker.sqlite3")
        );
    }

    #[test]
    fn raw_object_store_local_dir_trims_explicit_override() {
        use crate::runtime_env::resolve_raw_object_store_local_dir_from;
        assert_eq!(
            resolve_raw_object_store_local_dir_from("  /var/lib/pf/raw  ", |_| None).unwrap(),
            PathBuf::from("/var/lib/pf/raw")
        );
    }

    #[cfg(all(not(windows), not(target_os = "macos")))]
    #[test]
    fn raw_object_store_local_dir_defaults_under_xdg_data_home() {
        use crate::runtime_env::resolve_raw_object_store_local_dir_from;
        assert_eq!(
            resolve_raw_object_store_local_dir_from("", |key| {
                (key == "XDG_DATA_HOME").then(|| OsString::from("/tmp/prompt-ferry-data"))
            })
            .unwrap(),
            PathBuf::from("/tmp/prompt-ferry-data/prompt-ferry/raw-objects")
        );
    }
}
