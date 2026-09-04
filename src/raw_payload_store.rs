use std::{fmt, sync::Arc};

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder, path::Path};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

use crate::config::WorkerConfig;
use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};

/// Format marker for compressed raw payload objects: magic bytes followed by a
/// single format version byte. Objects written before this format existed are
/// plain JSON and remain readable.
const OBJECT_MAGIC: &[u8; 4] = b"PFR1";
const OBJECT_FORMAT_VERSION: u8 = 1;

/// Raw object-store backend selected by the administrator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RawObjectStoreBackend {
    #[default]
    Local,
    S3,
    Disabled,
}

impl RawObjectStoreBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::S3 => "s3",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "s3" => Some(Self::S3),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

fn default_s3_path_style() -> bool {
    true
}

/// Decrypted administrator-facing raw object-store configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct RawObjectStoreConfig {
    pub backend: RawObjectStoreBackend,
    pub local_dir: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_prefix: String,
    pub s3_allow_http: bool,
    #[serde(default = "default_s3_path_style")]
    #[schema(default = true)]
    pub s3_path_style: bool,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
}

impl Default for RawObjectStoreConfig {
    fn default() -> Self {
        Self {
            backend: RawObjectStoreBackend::Local,
            local_dir: String::new(),
            s3_endpoint: String::new(),
            s3_bucket: String::new(),
            s3_region: "auto".to_string(),
            s3_prefix: "prompt-ferry/raw".to_string(),
            s3_allow_http: false,
            s3_path_style: true,
            s3_access_key: None,
            s3_secret_key: None,
        }
    }
}

impl RawObjectStoreConfig {
    pub fn from_worker_config(config: &WorkerConfig) -> Self {
        let bucket = config.raw_object_store_bucket.trim();
        if bucket.is_empty() {
            Self {
                backend: RawObjectStoreBackend::Local,
                local_dir: config.raw_object_store_local_dir.clone(),
                s3_endpoint: config.raw_object_store_endpoint.clone(),
                s3_bucket: String::new(),
                s3_region: config.raw_object_store_region.clone(),
                s3_prefix: config.raw_object_store_prefix.clone(),
                s3_allow_http: config.raw_object_store_allow_http,
                s3_path_style: config.raw_object_store_path_style,
                s3_access_key: {
                    let v = config.raw_object_store_access_key.trim();
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    }
                },
                s3_secret_key: {
                    let v = config.raw_object_store_secret_key.trim();
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    }
                },
            }
        } else {
            Self {
                backend: RawObjectStoreBackend::S3,
                local_dir: config.raw_object_store_local_dir.clone(),
                s3_endpoint: config.raw_object_store_endpoint.clone(),
                s3_bucket: bucket.to_string(),
                s3_region: config.raw_object_store_region.clone(),
                s3_prefix: config.raw_object_store_prefix.clone(),
                s3_allow_http: config.raw_object_store_allow_http,
                s3_path_style: config.raw_object_store_path_style,
                s3_access_key: {
                    let v = config.raw_object_store_access_key.trim();
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    }
                },
                s3_secret_key: {
                    let v = config.raw_object_store_secret_key.trim();
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    }
                },
            }
        }
    }

    pub fn normalized(mut self) -> Self {
        self.backend = match self.backend {
            RawObjectStoreBackend::S3
            | RawObjectStoreBackend::Local
            | RawObjectStoreBackend::Disabled => self.backend,
        };
        self.local_dir = self.local_dir.trim().to_string();
        self.s3_endpoint = self.s3_endpoint.trim().to_string();
        self.s3_bucket = self.s3_bucket.trim().to_string();
        self.s3_region = {
            let v = self.s3_region.trim();
            if v.is_empty() {
                "auto".to_string()
            } else {
                v.to_string()
            }
        };
        self.s3_prefix = normalize_prefix(&self.s3_prefix);
        if self.backend == RawObjectStoreBackend::Disabled {
            // Disabled ignores S3 credentials but keep prefix normalized.
        }
        self.s3_access_key = self.s3_access_key.and_then(|v| {
            let t = v.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        });
        self.s3_secret_key = self.s3_secret_key.and_then(|v| {
            let t = v.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        });
        self
    }

    pub fn validate(&self) -> Result<()> {
        match self.backend {
            RawObjectStoreBackend::Disabled => Ok(()),
            RawObjectStoreBackend::Local => Ok(()),
            RawObjectStoreBackend::S3 => {
                if self.s3_bucket.trim().is_empty() {
                    return Err(anyhow!("s3 bucket is required for s3 backend"));
                }
                if self.s3_region.trim().is_empty() {
                    return Err(anyhow!("s3 region is required for s3 backend"));
                }
                Ok(())
            }
        }
    }

    pub fn build_store(&self) -> Result<Option<RawPayloadStore>> {
        self.validate()?;
        match self.backend {
            RawObjectStoreBackend::Disabled => Ok(None),
            RawObjectStoreBackend::Local => {
                let dir = crate::runtime_env::resolve_raw_object_store_local_dir(&self.local_dir)?;
                std::fs::create_dir_all(&dir).with_context(|| {
                    format!(
                        "failed to create local raw payload directory {}",
                        dir.display()
                    )
                })?;
                let store = Arc::new(object_store::local::LocalFileSystem::new_with_prefix(&dir)?);
                Ok(Some(RawPayloadStore {
                    store,
                    prefix: normalize_prefix(&self.s3_prefix),
                }))
            }
            RawObjectStoreBackend::S3 => {
                let s3 = build_s3_store_from_config(self)?;
                Ok(Some(RawPayloadStore {
                    store: Arc::new(s3),
                    prefix: normalize_prefix(&self.s3_prefix),
                }))
            }
        }
    }

    pub fn redacted_response(&self) -> RawObjectStoreSettingsResponse {
        RawObjectStoreSettingsResponse {
            backend: self.backend.clone(),
            local_dir: self.local_dir.clone(),
            s3_endpoint: self.s3_endpoint.clone(),
            s3_bucket: self.s3_bucket.clone(),
            s3_region: self.s3_region.clone(),
            s3_prefix: normalize_prefix(&self.s3_prefix),
            s3_allow_http: self.s3_allow_http,
            s3_path_style: self.s3_path_style,
            has_s3_access_key: self.s3_access_key.is_some(),
            has_s3_secret_key: self.s3_secret_key.is_some(),
        }
    }
}

/// Persisted JSON representation with encrypted S3 credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawObjectStorePersisted {
    pub backend: RawObjectStoreBackend,
    pub local_dir: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_prefix: String,
    pub s3_allow_http: bool,
    #[serde(default = "default_s3_path_style")]
    pub s3_path_style: bool,
    pub s3_access_key: Option<EncryptedSecretEnvelope>,
    pub s3_secret_key: Option<EncryptedSecretEnvelope>,
}

impl RawObjectStorePersisted {
    pub fn from_config(
        config: &RawObjectStoreConfig,
        manager: &RelaySecretManager,
    ) -> Result<Self> {
        let normalized = config.clone().normalized();
        let s3_access_key = match &normalized.s3_access_key {
            Some(value) if !value.trim().is_empty() => Some(manager.encrypt(value)?),
            _ => None,
        };
        let s3_secret_key = match &normalized.s3_secret_key {
            Some(value) if !value.trim().is_empty() => Some(manager.encrypt(value)?),
            _ => None,
        };
        Ok(Self {
            backend: normalized.backend,
            local_dir: normalized.local_dir,
            s3_endpoint: normalized.s3_endpoint,
            s3_bucket: normalized.s3_bucket,
            s3_region: normalized.s3_region,
            s3_prefix: normalized.s3_prefix,
            s3_allow_http: normalized.s3_allow_http,
            s3_path_style: normalized.s3_path_style,
            s3_access_key,
            s3_secret_key,
        })
    }

    pub fn into_config(self, manager: &RelaySecretManager) -> Result<RawObjectStoreConfig> {
        let s3_access_key = match self.s3_access_key {
            Some(envelope) => Some(manager.decrypt(&envelope)?),
            None => None,
        };
        let s3_secret_key = match self.s3_secret_key {
            Some(envelope) => Some(manager.decrypt(&envelope)?),
            None => None,
        };
        Ok(RawObjectStoreConfig {
            backend: self.backend,
            local_dir: self.local_dir,
            s3_endpoint: self.s3_endpoint,
            s3_bucket: self.s3_bucket,
            s3_region: self.s3_region,
            s3_prefix: self.s3_prefix,
            s3_allow_http: self.s3_allow_http,
            s3_path_style: self.s3_path_style,
            s3_access_key,
            s3_secret_key,
        }
        .normalized())
    }

    pub fn has_access_key(&self) -> bool {
        self.s3_access_key.is_some()
    }

    pub fn has_secret_key(&self) -> bool {
        self.s3_secret_key.is_some()
    }
}

/// Redacted API response for the admin settings surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct RawObjectStoreSettingsResponse {
    pub backend: RawObjectStoreBackend,
    pub local_dir: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_region: String,
    pub s3_prefix: String,
    pub s3_allow_http: bool,
    #[serde(default = "default_s3_path_style")]
    #[schema(default = true)]
    pub s3_path_style: bool,
    pub has_s3_access_key: bool,
    pub has_s3_secret_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct RawPayloadEnvelope {
    pub(crate) request_raw_json: Option<Value>,
    pub(crate) response_raw_body: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RawPayloadObject {
    pub(crate) object_key: String,
    pub(crate) size_bytes: i64,
    pub(crate) sha256: String,
    pub(crate) expires_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct RawPayloadStore {
    store: Arc<dyn ObjectStore>,
    prefix: String,
}

impl fmt::Debug for RawPayloadStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawPayloadStore")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl RawPayloadStore {
    /// Build the managed raw payload store: S3-compatible backend when a
    /// bucket is configured, otherwise a local filesystem directory. There is
    /// no PostgreSQL fallback; upload failures warn and drop that payload.
    pub(crate) fn from_config(config: &WorkerConfig) -> Result<Self> {
        let bucket = config.raw_object_store_bucket.trim();
        let store: Arc<dyn ObjectStore> = if bucket.is_empty() {
            let dir = crate::runtime_env::resolve_raw_object_store_local_dir(
                &config.raw_object_store_local_dir,
            )?;
            std::fs::create_dir_all(&dir).with_context(|| {
                format!(
                    "failed to create local raw payload directory {}",
                    dir.display()
                )
            })?;
            Arc::new(object_store::local::LocalFileSystem::new_with_prefix(&dir)?)
        } else {
            Arc::new(build_s3_store(config, bucket)?)
        };
        Ok(Self {
            store,
            prefix: normalize_prefix(&config.raw_object_store_prefix),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(store: Arc<dyn ObjectStore>, prefix: impl Into<String>) -> Self {
        Self {
            store,
            prefix: normalize_prefix(&prefix.into()),
        }
    }

    pub(crate) async fn put(
        &self,
        event_id: i64,
        created_at: DateTime<Utc>,
        payload: RawPayloadEnvelope,
        expires_at: DateTime<Utc>,
    ) -> Result<RawPayloadObject> {
        let object_key = self.object_key(event_id, created_at);
        let path = Path::from(object_key.as_str());
        let payload = self.merge_existing(&path, payload).await?;
        // Hash covers the exact stored bytes so readers detect corruption
        // before decompression.
        let bytes = encode_payload(&payload)?;
        let sha256 = sha256_hex(&bytes);
        let size_bytes = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        self.store
            .put(&path, PutPayload::from(Bytes::from(bytes)))
            .await
            .with_context(|| format!("failed to upload raw payload object {object_key}"))?;
        Ok(RawPayloadObject {
            object_key,
            size_bytes,
            sha256,
            expires_at,
        })
    }

    pub(crate) async fn get(
        &self,
        object_key: &str,
        expected_sha256: Option<&str>,
    ) -> Result<Option<RawPayloadEnvelope>> {
        let path = Path::from(object_key);
        let result = match self.store.get(&path).await {
            Ok(result) => result,
            Err(object_store::Error::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error).context("failed to read raw payload object"),
        };
        let bytes = result
            .bytes()
            .await
            .context("failed to read raw payload object bytes")?;
        if let Some(expected) = expected_sha256
            && sha256_hex(&bytes) != expected
        {
            return Err(anyhow!("raw payload object hash mismatch"));
        }
        decode_payload(&bytes)
            .context("failed to decode raw payload object")
            .map(Some)
    }

    pub async fn delete(&self, object_key: &str) -> Result<()> {
        self.store
            .delete(&Path::from(object_key))
            .await
            .with_context(|| format!("failed to delete raw payload object {object_key}"))
    }

    fn object_key(&self, event_id: i64, created_at: DateTime<Utc>) -> String {
        use chrono::Datelike;
        format!(
            "{}/{}/{:04}/{:02}/{:02}/{event_id}.bin",
            self.prefix,
            "events",
            created_at.year(),
            created_at.month(),
            created_at.day()
        )
    }

    /// Two-phase merge: request and response phases may write the same
    /// per-event object independently; whichever side is missing keeps the
    /// value already stored in the object.
    async fn merge_existing(
        &self,
        path: &Path,
        mut payload: RawPayloadEnvelope,
    ) -> Result<RawPayloadEnvelope> {
        let existing = match self.store.get(path).await {
            Ok(existing) => existing,
            Err(object_store::Error::NotFound { .. }) => return Ok(payload),
            Err(error) => return Err(error).context("failed to read existing raw payload object"),
        };
        let bytes = existing.bytes().await?;
        let existing = decode_payload(&bytes)?;
        if payload.request_raw_json.is_none() {
            payload.request_raw_json = existing.request_raw_json;
        }
        if payload.response_raw_body.is_none() {
            payload.response_raw_body = existing.response_raw_body;
        }
        Ok(payload)
    }
}

fn build_s3_store(config: &WorkerConfig, bucket: &str) -> Result<object_store::aws::AmazonS3> {
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(bucket)
        .with_region(config.raw_object_store_region.trim())
        .with_virtual_hosted_style_request(!config.raw_object_store_path_style);
    if !config.raw_object_store_endpoint.trim().is_empty() {
        builder = builder.with_endpoint(config.raw_object_store_endpoint.trim());
    }
    if !config.raw_object_store_access_key.trim().is_empty() {
        builder = builder.with_access_key_id(config.raw_object_store_access_key.trim());
    }
    if !config.raw_object_store_secret_key.trim().is_empty() {
        builder = builder.with_secret_access_key(config.raw_object_store_secret_key.trim());
    }
    if config.raw_object_store_allow_http {
        builder = builder.with_allow_http(true);
    }
    builder
        .build()
        .context("failed to build raw payload object store")
}

fn build_s3_store_from_config(
    config: &RawObjectStoreConfig,
) -> Result<object_store::aws::AmazonS3> {
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(config.s3_bucket.trim())
        .with_region(config.s3_region.trim())
        .with_virtual_hosted_style_request(!config.s3_path_style);
    if !config.s3_endpoint.trim().is_empty() {
        builder = builder.with_endpoint(config.s3_endpoint.trim());
    }
    if let Some(key) = config.s3_access_key.as_deref()
        && !key.trim().is_empty()
    {
        builder = builder.with_access_key_id(key.trim());
    }
    if let Some(key) = config.s3_secret_key.as_deref()
        && !key.trim().is_empty()
    {
        builder = builder.with_secret_access_key(key.trim());
    }
    if config.s3_allow_http {
        builder = builder.with_allow_http(true);
    }
    builder
        .build()
        .context("failed to build raw payload object store")
}

impl RawObjectStoreConfig {
    /// Build and validate the candidate store. Returns `None` for disabled.
    pub async fn build_and_validate(&self) -> Result<Option<RawPayloadStore>> {
        let store = self.build_store()?;
        if let Some(ref inner) = store {
            inner.validate_candidate().await?;
        }
        Ok(store)
    }
}

impl RawPayloadStore {
    /// Validate the live store is writable by performing a round-trip health check.
    pub async fn validate_candidate(&self) -> Result<()> {
        let test_key = format!("{}/.health/{}", self.prefix, uuid::Uuid::new_v4());
        let path = Path::from(test_key.as_str());
        self.store
            .put(&path, PutPayload::from_static(b"health"))
            .await
            .context("raw object store validation put failed")?;
        let _ = self
            .store
            .get(&path)
            .await
            .context("raw object store validation get failed")?;
        self.store
            .delete(&path)
            .await
            .context("raw object store validation delete failed")?;
        Ok(())
    }
}

fn encode_payload(payload: &RawPayloadEnvelope) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(payload).context("failed to encode raw payload object")?;
    let compressed =
        zstd::stream::encode_all(&json[..], 0).context("failed to compress raw payload object")?;
    let mut out = Vec::with_capacity(OBJECT_MAGIC.len() + 1 + compressed.len());
    out.extend_from_slice(OBJECT_MAGIC);
    out.push(OBJECT_FORMAT_VERSION);
    out.extend_from_slice(&compressed);
    Ok(out)
}

fn decode_payload(bytes: &[u8]) -> Result<RawPayloadEnvelope> {
    if !bytes.starts_with(OBJECT_MAGIC) {
        // Legacy objects were written as uncompressed JSON without a marker.
        return Ok(serde_json::from_slice(bytes)?);
    }
    let Some(version) = bytes.get(OBJECT_MAGIC.len()) else {
        return Err(anyhow!("raw payload object is missing its format version"));
    };
    if *version != OBJECT_FORMAT_VERSION {
        return Err(anyhow!(
            "unsupported raw payload object format version {version}"
        ));
    }
    let json = zstd::stream::decode_all(&bytes[OBJECT_MAGIC.len() + 1..])
        .context("failed to decompress raw payload object")?;
    Ok(serde_json::from_slice(&json)?)
}

fn normalize_prefix(prefix: &str) -> String {
    let prefix = prefix.trim().trim_matches('/');
    if prefix.is_empty() {
        "prompt-ferry/raw".to_string()
    } else {
        prefix.to_string()
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;

    fn test_store() -> (RawPayloadStore, Arc<InMemory>) {
        let inner = Arc::new(InMemory::new());
        (RawPayloadStore::new_for_test(inner.clone(), "raw"), inner)
    }

    fn sample_payload() -> RawPayloadEnvelope {
        let padding = "x".repeat(2048);
        RawPayloadEnvelope {
            request_raw_json: Some(serde_json::json!({"prompt": "hello", "pad": padding})),
            response_raw_body: Some("ok".to_string()),
        }
    }

    fn envelope(
        request_raw_json: Option<Value>,
        response_raw_body: Option<String>,
    ) -> RawPayloadEnvelope {
        RawPayloadEnvelope {
            request_raw_json,
            response_raw_body,
        }
    }

    async fn upload(
        store: &RawPayloadStore,
        event_id: i64,
        payload: RawPayloadEnvelope,
    ) -> RawPayloadObject {
        let expires_at = Utc::now() + chrono::Duration::days(3);
        store
            .put(event_id, Utc::now(), payload, expires_at)
            .await
            .expect("upload")
    }

    #[tokio::test]
    async fn put_merges_request_and_response_payloads() {
        let (store, _) = test_store();
        let first = upload(
            &store,
            7,
            envelope(Some(serde_json::json!({"prompt": "hello"})), None),
        )
        .await;
        assert!(first.size_bytes > 0);

        // A second phase writing only the response must preserve the request.
        let second = upload(&store, 7, envelope(None, Some("merged".to_string()))).await;
        let payload = store
            .get(&second.object_key, Some(&second.sha256))
            .await
            .expect("read")
            .expect("payload");
        assert!(payload.request_raw_json.is_some());
        assert_eq!(payload.response_raw_body.as_deref(), Some("merged"));
    }

    #[tokio::test]
    async fn get_rejects_hash_mismatch() {
        let (store, _) = test_store();
        let object = upload(&store, 8, sample_payload()).await;
        let error = store
            .get(&object.object_key, Some(&"0".repeat(64)))
            .await
            .expect_err("hash mismatch should fail closed");
        assert!(error.to_string().contains("hash mismatch"));
    }

    #[tokio::test]
    async fn legacy_uncompressed_objects_remain_readable() {
        let (store, inner) = test_store();
        let key = "legacy/event.json";
        let legacy =
            PutPayload::from_static(br#"{"request_raw_json":{"a":1},"response_raw_body":"old"}"#);
        inner
            .put(&Path::from(key), legacy)
            .await
            .expect("seed legacy object");
        let payload = store
            .get(key, None)
            .await
            .expect("legacy read")
            .expect("payload");
        assert_eq!(payload.request_raw_json, Some(serde_json::json!({"a": 1})));
        assert_eq!(payload.response_raw_body.as_deref(), Some("old"));
    }

    #[tokio::test]
    async fn corrupted_compressed_object_fails_closed() {
        let (store, inner) = test_store();
        let object = upload(&store, 11, sample_payload()).await;
        let mut bytes = inner
            .get(&Path::from(object.object_key.as_str()))
            .await
            .expect("object present")
            .bytes()
            .await
            .expect("object bytes")
            .to_vec();
        assert!(bytes.starts_with(OBJECT_MAGIC));
        assert_eq!(bytes[OBJECT_MAGIC.len()], OBJECT_FORMAT_VERSION);
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        inner
            .put(
                &Path::from(object.object_key.as_str()),
                PutPayload::from(bytes),
            )
            .await
            .expect("rewrite corrupted object");

        // Corrupted bytes pass verification (no expected hash) but must still
        // fail closed during decompression/decoding.
        let error = store
            .get(&object.object_key, None)
            .await
            .expect_err("corrupted object must not decode");
        assert!(error.to_string().contains("decode"), "{error:#}");
    }

    #[tokio::test]
    async fn unsupported_format_version_fails_closed() {
        let (store, inner) = test_store();
        let mut bytes = encode_payload(&sample_payload()).expect("encode");
        bytes[OBJECT_MAGIC.len()] = OBJECT_FORMAT_VERSION + 1;
        let key = "future/event.bin";
        inner
            .put(&Path::from(key), PutPayload::from(bytes))
            .await
            .expect("seed future object");
        let error = store.get(key, None).await.expect_err("must fail");
        assert!(format!("{error:#}").contains("format version"), "{error:#}");
    }

    #[test]
    fn local_store_is_selected_without_bucket() {
        use crate::config::WorkerConfig;
        let dir = std::env::temp_dir().join(format!("pf-raw-store-{}", uuid::Uuid::new_v4()));
        let config = WorkerConfig {
            raw_object_store_local_dir: dir.to_string_lossy().to_string(),
            ..WorkerConfig::default()
        };
        let store = RawPayloadStore::from_config(&config).expect("local store");
        assert_eq!(
            store.prefix,
            normalize_prefix(&config.raw_object_store_prefix)
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
