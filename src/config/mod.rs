mod types;

pub use config::{ReadOptions, read};
pub use types::*;

use crate::{naming::CONFIG_ENV_PREFIX, runtime_env};

pub fn read_app_config() -> Result<AppConfig, std::io::Error> {
    let app_name = runtime_env::select_config_app_name().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("failed to resolve config app name: {err}"),
        )
    })?;
    read(
        app_name,
        Some(ReadOptions::with_env_prefix(CONFIG_ENV_PREFIX)),
    )
}
