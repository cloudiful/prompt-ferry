mod dotenv;
mod types;

use config::{ReadOptions, read};
pub use dotenv::{DOTENV_PATH_ENV, load_repository_env};
pub use types::*;

use crate::{naming::NODE_CONFIG_ENV_PREFIX, runtime_env};

/// Nested environment prefixes of the removed role-scoped configuration.
const REMOVED_NESTED_ENV_PREFIXES: &[&str] = &["PROMPT_FERRY_RELAY__", "PROMPT_FERRY_WORKER__"];

/// The maintenance rekey still reads its legacy key from this explicit
/// variable, so the node startup classifier must not hide it.
const MAINTENANCE_LEGACY_KEY_ENV: &str = "PROMPT_FERRY_LEGACY_WORKER_CONFIG_ENCRYPTION_KEY";

/// Return the removed role-scoped environment key names present in `keys`.
///
/// Only names are classified: values are never read, returned or logged. The
/// maintenance legacy key is explicitly excluded because the rekey utility
/// still consumes it.
pub fn classify_removed_environment_keys<I>(keys: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut removed: Vec<String> = keys
        .into_iter()
        .filter(|key| key != MAINTENANCE_LEGACY_KEY_ENV)
        .filter(|key| {
            REMOVED_NESTED_ENV_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
        })
        .collect();
    removed.sort();
    removed.dedup();
    removed
}

fn removed_environment_keys() -> Vec<String> {
    classify_removed_environment_keys(
        std::env::vars_os().filter_map(|(key, _value)| key.into_string().ok()),
    )
}

fn reject_removed_environment_keys() -> Result<(), std::io::Error> {
    let removed = removed_environment_keys();
    if removed.is_empty() {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "obsolete role-scoped environment keys are set: {}. The node reads only the flat \
             PROMPT_FERRY__* namespace; remove these names before starting",
            removed.join(", ")
        ),
    ))
}

/// Read the node configuration from `PROMPT_FERRY__*`.
///
/// This is the startup configuration: it applies no host-local overlay, reads
/// no legacy `DATABASE_URL` alias, rejects the removed role-scoped environment
/// names, and validates the result.
pub fn read_node_config() -> Result<NodeConfig, std::io::Error> {
    reject_removed_environment_keys()?;
    let app_name = runtime_env::select_config_app_name().map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("failed to resolve config app name: {err}"),
        )
    })?;
    let node_config: NodeConfig = read(
        app_name,
        Some(ReadOptions::with_env_prefix(NODE_CONFIG_ENV_PREFIX)),
    )?;
    node_config
        .validate()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err.to_string()))?;
    Ok(node_config)
}

#[cfg(test)]
mod tests {
    use super::classify_removed_environment_keys;

    #[test]
    fn removed_role_environment_names_are_classified_by_name_only() {
        let removed = classify_removed_environment_keys([
            "PROMPT_FERRY_RELAY__CLIENT_TOKEN".to_string(),
            "PROMPT_FERRY_WORKER__VALKEY_URL".to_string(),
            "PROMPT_FERRY__DATABASE_URL".to_string(),
            "PROMPT_FERRY__VALKEY_URL".to_string(),
            "PROMPT_FERRY_WORKER_ADMIN_PORT".to_string(),
            "DATABASE_URL".to_string(),
            "PROMPT_FERRY_LEGACY_WORKER_CONFIG_ENCRYPTION_KEY".to_string(),
        ]);
        assert_eq!(
            removed,
            vec![
                "PROMPT_FERRY_RELAY__CLIENT_TOKEN".to_string(),
                "PROMPT_FERRY_WORKER__VALKEY_URL".to_string(),
            ]
        );
    }

    #[test]
    fn the_flat_namespace_is_never_reported_as_removed() {
        assert!(
            classify_removed_environment_keys([
                "PROMPT_FERRY__BIND".to_string(),
                "PROMPT_FERRY__MASTER_SECRET".to_string(),
                "PROMPT_FERRY__VALKEY_URL".to_string(),
            ])
            .is_empty()
        );
    }
}
