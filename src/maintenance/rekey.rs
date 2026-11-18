use anyhow::{Result, anyhow, bail};
use serde_json::Value;

use crate::relay_secrets::{
    EncryptedSecretEnvelope, RelaySecretManager, read_worker_config_key_file,
};
use std::path::Path;

const LEGACY_KEY_ENV: &str = "PROMPT_FERRY_LEGACY_WORKER_CONFIG_ENCRYPTION_KEY";
const FENCING_NOTICE: &str =
    "Registry-only fencing: stop old nodes; pre-protocol binaries cannot be fenced.";

enum LegacyKeySource<'a> {
    Environment(&'a str),
    File(&'a Path),
}

fn select_legacy_source<'a>(
    configured: Option<&'a str>,
    file: Option<&'a Path>,
) -> Result<LegacyKeySource<'a>> {
    let configured = configured.map(str::trim).filter(|value| !value.is_empty());
    match (configured, file) {
        (Some(_), Some(_)) => bail!("legacy key sources are ambiguous"),
        (Some(value), None) => Ok(LegacyKeySource::Environment(value)),
        (None, Some(path)) => Ok(LegacyKeySource::File(path)),
        (None, None) => bail!("legacy worker configuration key is required"),
    }
}

pub async fn run_rekey(args: crate::cli::RekeyArgs) -> Result<()> {
    let config =
        crate::config::read_node_config().map_err(|_| anyhow!("invalid node configuration"))?;
    if config.database_url.trim().is_empty() {
        bail!("node database URL is required for rekey");
    }
    let master = config
        .master_secret
        .as_ref()
        .ok_or_else(|| anyhow!("master secret is required"))?;
    let new = RelaySecretManager::from_key_bytes(*master.as_bytes());
    let configured = match std::env::var(LEGACY_KEY_ENV) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => bail!("invalid legacy key environment value"),
    };
    let old = match select_legacy_source(configured.as_deref(), args.legacy_key_file.as_deref())? {
        LegacyKeySource::Environment(value) => RelaySecretManager::from_base64(value)
            .map_err(|_| anyhow!("invalid legacy worker configuration key"))?,
        LegacyKeySource::File(path) => read_worker_config_key_file(path)?,
    };
    println!("{FENCING_NOTICE}");
    if old.same_key(&new) {
        println!("Old and new keys are identical; no changes made.");
        return Ok(());
    }
    if args.apply && !args.ack_stop_old_nodes {
        bail!("apply requires acknowledgement that old nodes are stopped");
    }
    let pool = crate::db::connect_with_max_connections(config.database_url.trim(), 1)
        .await
        .map_err(|_| anyhow!("failed to connect to rekey database"))?;
    let result =
        crate::db::rekey::rekey_database(&pool, args.apply, args.ack_stop_old_nodes, &old, &new)
            .await;
    pool.close().await;
    let report = result?;
    println!(
        "{}",
        if args.apply {
            "Rekey applied."
        } else {
            "Dry run: no changes made."
        }
    );
    match report.live_workers {
        Some(count) => println!("Live registered workers: {count}"),
        None => println!("Worker registry absent; live count unknown."),
    }
    for owner in report.owners {
        println!(
            "{}: {}, rows={}, envelopes={}",
            owner.table,
            if owner.present { "present" } else { "absent" },
            owner.rows,
            owner.envelopes
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RekeyDecision {
    Noop,
    DryRun,
    Apply,
}

pub fn decide_rekey(
    same_key: bool,
    apply: bool,
    acknowledged: bool,
    live_workers: Option<u64>,
) -> Result<RekeyDecision> {
    if same_key {
        return Ok(RekeyDecision::Noop);
    }
    if !apply {
        return Ok(RekeyDecision::DryRun);
    }
    if !acknowledged {
        bail!("apply requires acknowledgement that old nodes are stopped");
    }
    match live_workers {
        Some(0) => Ok(RekeyDecision::Apply),
        Some(_) => bail!("live workers prevent rekey"),
        None => bail!("worker registry is unavailable"),
    }
}

pub fn rekey_envelope(
    envelope: &EncryptedSecretEnvelope,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
) -> Result<EncryptedSecretEnvelope> {
    let plaintext = old
        .decrypt(envelope)
        .map_err(|_| anyhow!("invalid encrypted envelope"))?;
    let result = new
        .encrypt(&plaintext)
        .map_err(|_| anyhow!("could not encrypt envelope"))?;
    let verified = new
        .decrypt(&result)
        .map_err(|_| anyhow!("could not verify envelope"))?;
    if verified != plaintext {
        bail!("envelope verification failed");
    }
    Ok(result)
}

fn rekey_fields(
    value: &mut Value,
    fields: &[&str],
    old: &RelaySecretManager,
    new: &RelaySecretManager,
) -> Result<usize> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| anyhow!("invalid settings object"))?;
    let mut count = 0;
    for field in fields {
        let Some(value) = object.get_mut(*field) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let envelope: EncryptedSecretEnvelope = serde_json::from_value(value.clone())
            .map_err(|_| anyhow!("invalid encrypted settings envelope"))?;
        let updated = rekey_envelope(&envelope, old, new)?;
        let encoded = serde_json::to_value(updated)
            .map_err(|_| anyhow!("invalid encrypted settings envelope"))?;
        let target = value
            .as_object_mut()
            .ok_or_else(|| anyhow!("invalid encrypted settings envelope"))?;
        let replacement = encoded
            .as_object()
            .ok_or_else(|| anyhow!("invalid encrypted settings envelope"))?;
        for name in ["ciphertext", "nonce", "key_version"] {
            target.insert(name.to_owned(), replacement[name].clone());
        }
        count += 1;
    }
    Ok(count)
}

pub fn rekey_shared_valkey_value(
    value: &mut Value,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
) -> Result<usize> {
    if let Some(url) = value.get("url").filter(|value| !value.is_null()) {
        let envelope: EncryptedSecretEnvelope = serde_json::from_value(url.clone())
            .map_err(|_| anyhow!("invalid encrypted settings envelope"))?;
        let plaintext = old
            .decrypt(&envelope)
            .map_err(|_| anyhow!("invalid encrypted envelope"))?;
        let trimmed = plaintext.trim();
        if !trimmed.is_empty() {
            crate::db::validate_shared_valkey_url(trimmed)
                .map_err(|_| anyhow!("invalid encrypted Valkey URL"))?;
        }
    }
    rekey_fields(value, &["url"], old, new)
}

pub fn rekey_raw_object_store_value(
    value: &mut Value,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
) -> Result<usize> {
    rekey_fields(value, &["s3_access_key", "s3_secret_key"], old, new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_key_source_is_explicit_and_unambiguous() {
        let path = Path::new("keyfile");
        assert!(select_legacy_source(None, None).is_err());
        assert!(select_legacy_source(Some("  "), None).is_err());
        assert!(select_legacy_source(Some("key"), Some(path)).is_err());
        assert!(matches!(
            select_legacy_source(Some(" key "), None).unwrap(),
            LegacyKeySource::Environment("key")
        ));
        assert!(
            matches!(select_legacy_source(Some(" "), Some(path)).unwrap(), LegacyKeySource::File(selected) if selected == path)
        );
    }

    #[test]
    fn decisions_fail_closed_and_same_key_is_noop() {
        assert_eq!(
            decide_rekey(true, true, false, None).unwrap(),
            RekeyDecision::Noop
        );
        assert_eq!(
            decide_rekey(false, false, false, None).unwrap(),
            RekeyDecision::DryRun
        );
        assert!(decide_rekey(false, true, false, Some(0)).is_err());
        assert!(decide_rekey(false, true, true, None).is_err());
        assert!(decide_rekey(false, true, true, Some(1)).is_err());
        assert_eq!(
            decide_rekey(false, true, true, Some(0)).unwrap(),
            RekeyDecision::Apply
        );
    }

    #[test]
    fn raw_store_preserves_unrelated_values() {
        let old = RelaySecretManager::from_key_bytes([1; 32]);
        let new = RelaySecretManager::from_key_bytes([2; 32]);
        let envelope = serde_json::to_value(old.encrypt("access").unwrap()).unwrap();
        let mut value =
            json!({"s3_access_key": envelope, "s3_secret_key": null, "future": {"keep": 7}});
        assert_eq!(
            rekey_raw_object_store_value(&mut value, &old, &new).unwrap(),
            1
        );
        assert_eq!(value["future"]["keep"], 7);
        assert!(value["s3_secret_key"].is_null());
        let decoded = serde_json::from_value(value["s3_access_key"].clone()).unwrap();
        assert_eq!(new.decrypt(&decoded).unwrap(), "access");
    }

    #[test]
    fn envelope_and_json_preserve_content() {
        let old = RelaySecretManager::from_key_bytes([1; 32]);
        let new = RelaySecretManager::from_key_bytes([2; 32]);
        let original = old.encrypt("redis://localhost:6379").unwrap();
        let updated = rekey_envelope(&original, &old, &new).unwrap();
        assert_eq!(new.decrypt(&updated).unwrap(), "redis://localhost:6379");
        assert!(old.decrypt(&updated).is_err());
        assert_eq!(updated.key_version, 1);
        assert_ne!(updated.nonce, original.nonce);
        let mut invalid = original.clone();
        invalid.key_version = 2;
        assert!(rekey_envelope(&invalid, &old, &new).is_err());
        let mut envelope = serde_json::to_value(original).unwrap();
        envelope["future"] = json!("keep");
        let mut value =
            json!({"url": envelope, "revision": 42, "ttl_seconds": 123, "unknown": true});
        assert_eq!(
            rekey_shared_valkey_value(&mut value, &old, &new).unwrap(),
            1
        );
        assert_eq!(value["revision"], 42);
        assert_eq!(value["ttl_seconds"], 123);
        assert_eq!(value["unknown"], true);
        assert_eq!(value["url"]["future"], "keep");
        let decoded = serde_json::from_value(value["url"].clone()).unwrap();
        assert_eq!(new.decrypt(&decoded).unwrap(), "redis://localhost:6379");
    }
}
