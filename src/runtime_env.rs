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

fn default_config_path(app_name: &str) -> Result<PathBuf> {
    let app_name = Path::new(app_name);
    validate_app_name(app_name)?;
    Ok(config_root_from(|key| env::var_os(key))?
        .join(app_name)
        .join("config.toml"))
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
    use super::resolve_database_url;

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
}
