pub mod binds;
mod dotenv;
pub(crate) mod host_startup;
pub mod integrated_bridge;
pub(crate) mod integrated_startup;
mod types;

pub use config::{ReadOptions, read};
pub use dotenv::{DOTENV_PATH_ENV, load_repository_env};
pub use integrated_startup::IntegratedStartup;
pub use types::*;

use crate::{naming::CONFIG_ENV_PREFIX, runtime_env};

/// Canonical PostgreSQL URL environment variable. An explicit `--database-url`
/// argument still wins over it via `merge_args`, and the config file field
/// `worker.database_url` remains the fallback when the variable is unset.
const DATABASE_URL_ENV: &str = "DATABASE_URL";

pub fn read_app_config() -> Result<AppConfig, std::io::Error> {
    let app_name = runtime_env::select_config_app_name().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("failed to resolve config app name: {err}"),
        )
    })?;
    let mut app_config: AppConfig = read(
        app_name,
        Some(ReadOptions::with_env_prefix(CONFIG_ENV_PREFIX)),
    )?;
    if let Some(database_url) = database_url_from_env() {
        app_config.worker.database_url = database_url;
    }
    Ok(app_config)
}

fn database_url_from_env() -> Option<String> {
    std::env::var(DATABASE_URL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
