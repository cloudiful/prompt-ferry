use crate::naming::CONFIG_APP_NAME;
use anyhow::{Result, anyhow};
pub use db_init::DatabaseUrlResolution;
use db_init::{load_dotenv_if_exists, resolve_database_url as resolve_shared_database_url};
use std::{
    env,
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

const DATABASE_URL_ENV_KEYS: &[&str] = &[
    "PROMPT_FERRY_DEV_DATABASE_URL",
    "PROMPT_FERRY_WORKER__DATABASE_URL",
    "DATABASE_URL",
];

pub fn load_dotenv(path: impl AsRef<Path>) -> Result<()> {
    load_dotenv_if_exists(path)
}

pub fn resolve_database_url(from_arg: Option<String>) -> Result<DatabaseUrlResolution> {
    resolve_shared_database_url(from_arg, DATABASE_URL_ENV_KEYS, || {
        let config = crate::config::read_app_config()
            .map_err(|error| anyhow!(error).context("failed to load prompt-ferry config"))?;
        Ok(Some(config.worker.database_url))
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
    use super::{resolve_database_url, resolve_standalone_database_path_from};
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn database_url_resolution_prefers_arg_then_new_then_database_url() {
        unsafe {
            std::env::remove_var("PROMPT_FERRY_DEV_DATABASE_URL");
            std::env::remove_var("PROMPT_FERRY_WORKER__DATABASE_URL");
            std::env::remove_var("DATABASE_URL");
        }

        assert_eq!(
            resolve_database_url(Some(" postgres://arg ".to_string()))
                .unwrap()
                .database_url,
            "postgres://arg"
        );

        unsafe { std::env::set_var("PROMPT_FERRY_DEV_DATABASE_URL", "postgres://new-dev") };
        assert_eq!(
            resolve_database_url(None).unwrap().database_url,
            "postgres://new-dev"
        );

        unsafe {
            std::env::remove_var("PROMPT_FERRY_DEV_DATABASE_URL");
            std::env::set_var("PROMPT_FERRY_WORKER__DATABASE_URL", "postgres://worker");
        }
        assert_eq!(
            resolve_database_url(None).unwrap().database_url,
            "postgres://worker"
        );

        unsafe {
            std::env::remove_var("PROMPT_FERRY_DEV_DATABASE_URL");
            std::env::remove_var("PROMPT_FERRY_WORKER__DATABASE_URL");
            std::env::set_var("DATABASE_URL", "postgres://default");
        }
        assert_eq!(
            resolve_database_url(None).unwrap().database_url,
            "postgres://default"
        );

        unsafe {
            std::env::remove_var("PROMPT_FERRY_DEV_DATABASE_URL");
            std::env::remove_var("PROMPT_FERRY_WORKER__DATABASE_URL");
            std::env::remove_var("DATABASE_URL");
        }
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
