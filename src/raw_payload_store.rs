use std::{fmt, sync::Arc};

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder, path::Path};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::WorkerConfig;

/// Format marker for compressed raw payload objects: magic bytes followed by a
/// single format version byte. Objects written before this format existed are
/// plain JSON and remain readable.
const OBJECT_MAGIC: &[u8; 4] = b"PFR1";
const OBJECT_FORMAT_VERSION: u8 = 1;

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
        .with_region(config.raw_object_store_region.trim());
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
