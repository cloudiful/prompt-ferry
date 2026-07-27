use std::{fmt, sync::Arc};

use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder, path::Path};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::config::WorkerConfig;

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
    pub(crate) fn from_config(config: &WorkerConfig) -> Result<Option<Self>> {
        let bucket = config.raw_object_store_bucket.trim();
        if bucket.is_empty() {
            return Ok(None);
        }
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
        let store = builder
            .build()
            .context("failed to build raw payload object store")?;
        Ok(Some(Self {
            store: Arc::new(store),
            prefix: normalize_prefix(&config.raw_object_store_prefix),
        }))
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
        let bytes = serde_json::to_vec(&payload).context("failed to encode raw payload object")?;
        let sha256 = sha256_hex(&bytes);
        self.store
            .put(&path, PutPayload::from(Bytes::from(bytes.clone())))
            .await
            .with_context(|| format!("failed to upload raw payload object {object_key}"))?;
        Ok(RawPayloadObject {
            object_key,
            size_bytes: i64::try_from(bytes.len()).unwrap_or(i64::MAX),
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
        serde_json::from_slice(&bytes)
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
        format!(
            "{}/{}/{:04}/{:02}/{:02}/{event_id}.json",
            self.prefix,
            "events",
            created_at.year(),
            created_at.month(),
            created_at.day()
        )
    }

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
        let existing = serde_json::from_slice::<RawPayloadEnvelope>(&bytes)?;
        if payload.request_raw_json.is_none() {
            payload.request_raw_json = existing.request_raw_json;
        }
        if payload.response_raw_body.is_none() {
            payload.response_raw_body = existing.response_raw_body;
        }
        Ok(payload)
    }
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

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;

    #[tokio::test]
    async fn put_merges_request_and_response_payloads() {
        let store = RawPayloadStore::new_for_test(Arc::new(InMemory::new()), "raw");
        let created_at = Utc::now();
        let expires_at = created_at + chrono::Duration::days(3);
        let first = store
            .put(
                7,
                created_at,
                RawPayloadEnvelope {
                    request_raw_json: Some(serde_json::json!({"prompt": "hello"})),
                    response_raw_body: None,
                },
                expires_at,
            )
            .await
            .expect("first upload");
        store
            .put(
                7,
                created_at,
                RawPayloadEnvelope {
                    request_raw_json: None,
                    response_raw_body: Some("ok".to_string()),
                },
                expires_at,
            )
            .await
            .expect("second upload");
        let payload = store
            .get(&first.object_key, None)
            .await
            .expect("read")
            .expect("payload");
        assert!(payload.request_raw_json.is_some());
        assert_eq!(payload.response_raw_body.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn get_rejects_hash_mismatch() {
        let store = RawPayloadStore::new_for_test(Arc::new(InMemory::new()), "raw");
        let object = store
            .put(
                8,
                Utc::now(),
                RawPayloadEnvelope {
                    request_raw_json: Some(serde_json::json!({"prompt": "hello"})),
                    response_raw_body: None,
                },
                Utc::now() + chrono::Duration::days(3),
            )
            .await
            .expect("upload");

        let error = store
            .get(&object.object_key, Some(&"0".repeat(64)))
            .await
            .expect_err("hash mismatch should fail closed");
        assert!(error.to_string().contains("hash mismatch"));
    }
}
