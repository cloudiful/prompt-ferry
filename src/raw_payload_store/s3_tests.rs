//! Real S3 integration tests over the production store builder.
//!
//! These tests run against the dedicated test bucket provisioned for this
//! project, through `RawObjectStoreConfig::build_store` — the same production
//! path the worker uses — and never through the `cfg(test)` duplicate
//! builders. Every object lives under one unique `run-<uuid>` prefix per test,
//! each key is registered before its first PUT, and cleanup deletes exactly
//! the registered keys, so a green run leaves zero owned objects behind.
//!
//! The guard below mirrors `scripts/ci/test_s3_target.py` without sharing
//! code with it: all eight dedicated settings must be present and agree on one
//! confirmed endpoint-root/bucket before any network happens. A run with no S3
//! settings reports an explicit skip (focused runs never reach the shell
//! preflight); a partial or mismatched setting fails before any I/O, and no
//! AWS default or ambient credential fallback is consulted. Credentials never
//! reach test output: refusals name keys, never values.
//!
//! A SIGKILLed or host-crashed run cannot execute its cleanup, so its
//! `run-<uuid>` prefix is left for the operator; nothing here deletes unknown
//! keys or lists any bucket.

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use futures::FutureExt;

use super::{RawObjectStoreBackend, RawObjectStoreConfig, RawPayloadEnvelope, RawPayloadStore};

const ENDPOINT_KEY: &str = "PROMPT_FERRY_TEST_S3_ENDPOINT";
const BUCKET_KEY: &str = "PROMPT_FERRY_TEST_S3_BUCKET";
const REGION_KEY: &str = "PROMPT_FERRY_TEST_S3_REGION";
const ACCESS_KEY_KEY: &str = "PROMPT_FERRY_TEST_S3_ACCESS_KEY";
const SECRET_KEY_KEY: &str = "PROMPT_FERRY_TEST_S3_SECRET_KEY";
const ALLOW_HTTP_KEY: &str = "PROMPT_FERRY_TEST_S3_ALLOW_HTTP";
const PATH_STYLE_KEY: &str = "PROMPT_FERRY_TEST_S3_PATH_STYLE";
const CONFIRMED_TARGET_KEY: &str = "PROMPT_FERRY_TEST_S3_CONFIRMED_TARGET";

const EXPECTED_BUCKET: &str = "prompt-ferry-test";

/// Per-operation network bound: a hung endpoint fails the test instead of
/// hanging the suite in SDK retries.
const OP_TIMEOUT: Duration = Duration::from_secs(30);

enum S3Guard {
    Ready(RawObjectStoreConfig),
    Skip,
    Refuse(String),
}

fn parse_bool(value: &str, key: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{key} must read exactly true or false")),
    }
}

/// Validates the service-root endpoint and returns its normalized root.
/// Normalization strips trailing slashes only; userinfo, query strings,
/// fragments, and non-root paths are refused, and the root is rebuilt from
/// the parsed parts so a hostile value is never echoed.
fn endpoint_root(value: &str) -> Result<(String, String), String> {
    let raw = value.trim();
    if raw.is_empty() {
        return Err(format!(
            "{ENDPOINT_KEY} is required for the S3 integration tests"
        ));
    }
    let Some((scheme, remainder)) = raw.split_once("://") else {
        return Err(format!("{ENDPOINT_KEY} must use an http or https endpoint"));
    };
    if scheme != "http" && scheme != "https" {
        return Err(format!("{ENDPOINT_KEY} must use an http or https endpoint"));
    }
    if remainder.contains('@') {
        return Err(format!("{ENDPOINT_KEY} must not carry userinfo"));
    }
    if remainder.contains('?') {
        return Err(format!("{ENDPOINT_KEY} must not carry query parameters"));
    }
    if remainder.contains('#') {
        return Err(format!("{ENDPOINT_KEY} must not carry a fragment"));
    }
    let (authority, path) = match remainder.find('/') {
        Some(index) => (&remainder[..index], &remainder[index..]),
        None => (remainder, ""),
    };
    if !path.trim_matches('/').is_empty() {
        return Err(format!(
            "{ENDPOINT_KEY} must address the service root, not a path"
        ));
    }
    let (host, port) = match authority.find(':') {
        Some(index) => (&authority[..index], Some(&authority[index + 1..])),
        None => (authority, None),
    };
    if host.is_empty() {
        return Err(format!("{ENDPOINT_KEY} must name a host explicitly"));
    }
    let host = host.to_lowercase();
    if let Some(port) = port {
        let valid = !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok()
            && port
                .parse::<u16>()
                .map(|number| number > 0)
                .unwrap_or(false);
        if !valid {
            return Err(format!("{ENDPOINT_KEY} has an invalid port"));
        }
        Ok((scheme.to_string(), format!("{scheme}://{host}:{port}")))
    } else {
        Ok((scheme.to_string(), format!("{scheme}://{host}")))
    }
}

fn guard_from_map(vars: &HashMap<String, String>) -> S3Guard {
    let get = |key: &str| {
        vars.get(key)
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };
    let values = [
        get(ENDPOINT_KEY),
        get(BUCKET_KEY),
        get(REGION_KEY),
        get(ACCESS_KEY_KEY),
        get(SECRET_KEY_KEY),
        get(ALLOW_HTTP_KEY),
        get(PATH_STYLE_KEY),
        get(CONFIRMED_TARGET_KEY),
    ];
    if values.iter().all(|value| value.is_empty()) {
        return S3Guard::Skip;
    }

    let fail = |message: String| S3Guard::Refuse(message);
    if values.iter().any(|value| value.is_empty()) {
        let missing = [
            ENDPOINT_KEY,
            BUCKET_KEY,
            REGION_KEY,
            ACCESS_KEY_KEY,
            SECRET_KEY_KEY,
            ALLOW_HTTP_KEY,
            PATH_STYLE_KEY,
            CONFIRMED_TARGET_KEY,
        ]
        .into_iter()
        .zip(values.iter())
        .filter(|(_, value)| value.is_empty())
        .map(|(key, _)| key)
        .collect::<Vec<_>>()
        .join(", ");
        return fail(format!(
            "the S3 integration tests need every dedicated setting; missing: {missing}"
        ));
    }

    let (scheme, root) = match endpoint_root(&values[0]) {
        Ok(parsed) => parsed,
        Err(message) => return fail(message),
    };
    if values[1] != EXPECTED_BUCKET {
        return fail(format!(
            "{BUCKET_KEY} must name the dedicated test bucket; \
             {CONFIRMED_TARGET_KEY} stays the only authority for where tests run"
        ));
    }
    if values[2].is_empty() {
        return fail(format!("{REGION_KEY} must name a region explicitly"));
    }
    let allow_http = match parse_bool(&values[5], ALLOW_HTTP_KEY) {
        Ok(parsed) => parsed,
        Err(message) => return fail(message),
    };
    let path_style = match parse_bool(&values[6], PATH_STYLE_KEY) {
        Ok(parsed) => parsed,
        Err(message) => return fail(message),
    };
    if scheme == "http" && !allow_http {
        return fail(format!(
            "a plain-HTTP endpoint needs {ALLOW_HTTP_KEY} set to true; \
             plaintext without an explicit opt-in is refused"
        ));
    }
    if values[7] != format!("{root}/{}", values[1]) {
        return fail(format!(
            "{CONFIRMED_TARGET_KEY} must read exactly the confirmed \
             endpoint-root/bucket; a test target must be the one an operator confirmed"
        ));
    }

    S3Guard::Ready(RawObjectStoreConfig {
        backend: RawObjectStoreBackend::S3,
        local_dir: String::new(),
        s3_endpoint: root,
        s3_bucket: values[1].clone(),
        s3_region: values[2].clone(),
        s3_prefix: format!("run-{}", uuid::Uuid::new_v4()),
        s3_allow_http: allow_http,
        s3_path_style: path_style,
        s3_access_key: Some(values[3].clone()),
        s3_secret_key: Some(values[4].clone()),
    })
}

fn guard_from_env() -> S3Guard {
    let vars: HashMap<String, String> = std::env::vars().collect();
    guard_from_map(&vars)
}

/// Builds one store handle for the dedicated bucket, or `None` when the run
/// carries no S3 settings at all. A partial or mismatched setting panics
/// before any network happens; the message names the key, never a value.
fn setup_store() -> Option<RawPayloadStore> {
    match guard_from_env() {
        S3Guard::Skip => {
            println!(
                "S3_TEST_SKIP: no dedicated S3 settings; skipping real S3 tests \
                 (focused run without the dedicated target)"
            );
            None
        }
        S3Guard::Refuse(reason) => panic!("{reason}"),
        S3Guard::Ready(config) => {
            let store = config
                .build_store()
                .expect("the dedicated S3 target must build a store")
                .expect("the S3 backend always builds a store");
            Some(store)
        }
    }
}

/// Bounds one store operation so a hung endpoint fails instead of hanging.
async fn io<T>(label: &'static str, operation: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(OP_TIMEOUT, operation)
        .await
        .map_err(|_| anyhow!("S3 {label} timed out"))?
}

/// Tracks exactly the keys one test owns. Keys are registered before their
/// first PUT; cleanup deletes only registered keys, and a cleanup failure is
/// reported, never swallowed. The key list is shared so cleanup still runs
/// when the body panics: registration happens-before any later failure, and
/// the outer owner keeps every key registered so far.
#[derive(Clone)]
struct TestCleanup {
    store: RawPayloadStore,
    keys: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl TestCleanup {
    fn new(store: RawPayloadStore) -> Self {
        Self {
            store,
            keys: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn register(&self, key: String) {
        let mut keys = self.keys.lock().expect("cleanup registry lock");
        if !keys.contains(&key) {
            keys.push(key);
        }
    }

    async fn run(&self) -> Result<()> {
        let keys = self.keys.lock().expect("cleanup registry lock").clone();
        let mut failures = Vec::new();
        for key in &keys {
            if let Err(error) = io("cleanup delete", self.store.delete(key)).await {
                failures.push(format!("{key}: {error:#}"));
            } else {
                match io("cleanup verify get", self.store.get(key, None)).await {
                    Ok(None) => {}
                    Ok(Some(_)) => {
                        failures.push(format!("{key}: owned object remains after cleanup"))
                    }
                    Err(_) => failures.push(format!("{key}: cleanup absence verification failed")),
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(
                "S3 cleanup failed for {} key(s): {}",
                failures.len(),
                failures.join("; ")
            ))
        }
    }
}

/// Runs one guarded case: builds the dedicated store (or skips), runs the
/// body with an owned handle and a shared cleanup tracker, always awaits
/// cleanup, then reports the body's outcome. A body panic resumes after
/// cleanup; a body error returns after cleanup, with a cleanup failure
/// appended rather than replacing the original error. Everything the body
/// touches is owned, so no borrow outlives the case.
async fn run_s3_case<F>(body: impl FnOnce(RawPayloadStore, TestCleanup) -> F) -> Result<()>
where
    F: Future<Output = Result<()>>,
{
    let Some(store) = setup_store() else {
        return Ok(());
    };
    let cleanup = TestCleanup::new(store.clone());
    let outcome = AssertUnwindSafe(body(store, cleanup.clone()))
        .catch_unwind()
        .await;
    let cleanup_outcome = cleanup.run().await;
    match outcome {
        Err(payload) => {
            if let Err(error) = cleanup_outcome {
                eprintln!("S3_TEST_CLEANUP_FAILED: {error:#}");
            }
            std::panic::resume_unwind(payload);
        }
        Ok(Err(error)) => {
            if let Err(cleanup_error) = cleanup_outcome {
                return Err(anyhow!("{error:#}; cleanup also failed: {cleanup_error:#}"));
            }
            Err(error)
        }
        Ok(Ok(())) => cleanup_outcome,
    }
}

fn full_envelope() -> RawPayloadEnvelope {
    RawPayloadEnvelope {
        request_raw_json: Some(serde_json::json!({"phase": "request", "prompt": "s3-probe"})),
        response_raw_body: Some("s3-response".to_string()),
    }
}

fn expires_at() -> DateTime<Utc> {
    Utc::now() + chrono::Duration::days(30)
}

#[tokio::test]
async fn s3_roundtrip_preserves_compressed_payload_and_metadata() {
    run_s3_case(|store, cleanup| async move {
        let created_at = Utc::now();
        let key = store.object_key(5101, created_at);
        cleanup.register(key.clone());
        let object = io(
            "put",
            store.put(5101, created_at, full_envelope(), expires_at()),
        )
        .await
        .context("S3 put must succeed")?;
        assert!(object.size_bytes > 0, "stored object must be non-empty");
        assert_eq!(object.sha256.len(), 64, "stored object must carry a hash");

        let payload = io("get", store.get(&object.object_key, Some(&object.sha256)))
            .await
            .context("S3 get must succeed")?
            .expect("the object just written must read back");
        assert_eq!(
            payload.request_raw_json,
            Some(serde_json::json!({"phase": "request", "prompt": "s3-probe"}))
        );
        assert_eq!(payload.response_raw_body.as_deref(), Some("s3-response"));
        Ok(())
    })
    .await
    .expect("S3 roundtrip must succeed");
}

#[tokio::test]
async fn s3_successive_puts_merge_request_and_response() {
    run_s3_case(|store, cleanup| async move {
        let created_at = Utc::now();
        let key = store.object_key(5102, created_at);
        cleanup.register(key.clone());
        io(
            "first put",
            store.put(
                5102,
                created_at,
                RawPayloadEnvelope {
                    request_raw_json: Some(serde_json::json!({"phase": "request"})),
                    response_raw_body: None,
                },
                expires_at(),
            ),
        )
        .await
        .context("S3 first put must succeed")?;
        let merged = io(
            "second put",
            store.put(
                5102,
                created_at,
                RawPayloadEnvelope {
                    request_raw_json: None,
                    response_raw_body: Some("merged-response".to_string()),
                },
                expires_at(),
            ),
        )
        .await
        .context("S3 second put must succeed")?;

        let payload = io("get", store.get(&merged.object_key, Some(&merged.sha256)))
            .await
            .context("S3 get must succeed")?
            .expect("the merged object must read back");
        assert_eq!(
            payload.request_raw_json,
            Some(serde_json::json!({"phase": "request"}))
        );
        assert_eq!(
            payload.response_raw_body.as_deref(),
            Some("merged-response")
        );
        Ok(())
    })
    .await
    .expect("S3 merge must succeed");
}

#[tokio::test]
async fn s3_wrong_checksum_fails_closed() {
    run_s3_case(|store, cleanup| async move {
        let created_at = Utc::now();
        let key = store.object_key(5103, created_at);
        cleanup.register(key.clone());
        let object = io(
            "put",
            store.put(5103, created_at, full_envelope(), expires_at()),
        )
        .await
        .context("S3 put must succeed")?;
        let error = io(
            "mismatched get",
            store.get(&object.object_key, Some(&"0".repeat(64))),
        )
        .await
        .expect_err("a wrong checksum must fail closed");
        assert!(
            error.to_string().contains("hash mismatch"),
            "unexpected error: {error:#}"
        );
        Ok(())
    })
    .await
    .expect("S3 checksum case must succeed");
}

#[tokio::test]
async fn s3_missing_key_reads_none() {
    run_s3_case(|store, cleanup| async move {
        // Registered before use like every other key; the object is simply
        // never written, so cleanup's delete stays idempotent.
        let key = store.object_key(5104, Utc::now());
        cleanup.register(key.clone());
        let payload = io("missing get", store.get(&key, None))
            .await
            .context("S3 missing get must succeed")?;
        assert!(payload.is_none(), "a missing key must read None");
        Ok(())
    })
    .await
    .expect("S3 missing-key case must succeed");
}

#[tokio::test]
async fn s3_delete_is_idempotent_and_hides_the_key() {
    run_s3_case(|store, cleanup| async move {
        let created_at = Utc::now();
        let key = store.object_key(5105, created_at);
        cleanup.register(key.clone());
        io(
            "put",
            store.put(5105, created_at, full_envelope(), expires_at()),
        )
        .await
        .context("S3 put must succeed")?;
        io("delete", store.delete(&key))
            .await
            .context("S3 delete must succeed")?;
        let payload = io("get after delete", store.get(&key, None))
            .await
            .context("S3 get after delete must succeed")?;
        assert!(payload.is_none(), "a deleted key must read None");
        // S3 deletes are idempotent: removing the same key again succeeds.
        io("second delete", store.delete(&key))
            .await
            .context("S3 second delete must succeed")?;
        Ok(())
    })
    .await
    .expect("S3 delete case must succeed");
}

#[tokio::test]
async fn s3_body_failure_still_cleans_up_and_reports() {
    use std::sync::mpsc::channel;

    let (sender, receiver) = channel();
    let outcome = AssertUnwindSafe(run_s3_case(|store, cleanup| async move {
        let created_at = Utc::now();
        let key = store.object_key(5106, created_at);
        cleanup.register(key.clone());
        sender
            .send(key)
            .expect("the test must observe the registered key");
        io(
            "put",
            store.put(5106, created_at, full_envelope(), expires_at()),
        )
        .await
        .context("S3 put must succeed")?;
        Err::<(), _>(anyhow!("intentional body failure for cleanup verification"))
    }))
    .catch_unwind()
    .await
    .expect("the wrapper itself must not panic");
    let Ok(key) = receiver.try_recv() else {
        println!("S3_TEST_SKIP: no dedicated S3 settings; skipping real S3 tests");
        return;
    };
    let error = outcome.expect_err("the body failure must propagate");
    assert!(
        error.to_string().contains("intentional body failure"),
        "unexpected error: {error:#}"
    );

    // Cleanup ran before the error returned: the registered key is gone.
    let verifier = setup_store().expect("the dedicated target is configured");
    let payload = io("verify get", verifier.get(&key, None))
        .await
        .expect("the verify read must succeed");
    assert!(
        payload.is_none(),
        "cleanup must have deleted the registered key"
    );
}

#[tokio::test]
async fn s3_panic_still_cleans_up_and_propagates() {
    use std::sync::mpsc::channel;

    let (sender, receiver) = channel();
    let outcome = AssertUnwindSafe(run_s3_case(|store, cleanup| async move {
        let created_at = Utc::now();
        let key = store.object_key(5107, created_at);
        cleanup.register(key.clone());
        sender
            .send(key)
            .expect("the test must observe the registered key");
        io(
            "put",
            store.put(5107, created_at, full_envelope(), expires_at()),
        )
        .await
        .expect("S3 put must succeed");
        panic!("intentional panic for cleanup verification");
    }))
    .catch_unwind()
    .await;
    let Ok(key) = receiver.try_recv() else {
        println!("S3_TEST_SKIP: no dedicated S3 settings; skipping real S3 tests");
        return;
    };
    let panicked = outcome.expect_err("the wrapper must surface the panic, not Ok");
    let message = panicked
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panicked.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        message.contains("intentional panic"),
        "the original panic must propagate after cleanup"
    );

    let verifier = setup_store().expect("the dedicated target is configured");
    let payload = io("verify get", verifier.get(&key, None))
        .await
        .expect("the verify read must succeed");
    assert!(
        payload.is_none(),
        "cleanup must have deleted the registered key"
    );
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    fn valid_vars() -> HashMap<String, String> {
        [
            (
                ENDPOINT_KEY.to_string(),
                "http://s3-test.example.test:9002".to_string(),
            ),
            (BUCKET_KEY.to_string(), EXPECTED_BUCKET.to_string()),
            (REGION_KEY.to_string(), "us-east-1".to_string()),
            (ACCESS_KEY_KEY.to_string(), "s3-fixture-access".to_string()),
            (SECRET_KEY_KEY.to_string(), "s3-fixture-secret".to_string()),
            (ALLOW_HTTP_KEY.to_string(), "true".to_string()),
            (PATH_STYLE_KEY.to_string(), "true".to_string()),
            (
                CONFIRMED_TARGET_KEY.to_string(),
                "http://s3-test.example.test:9002/prompt-ferry-test".to_string(),
            ),
        ]
        .into_iter()
        .collect()
    }

    fn ready(vars: &HashMap<String, String>) -> RawObjectStoreConfig {
        match guard_from_map(vars) {
            S3Guard::Ready(config) => config,
            S3Guard::Skip => panic!("a complete fixture must not skip"),
            S3Guard::Refuse(reason) => panic!("a complete fixture must not refuse: {reason}"),
        }
    }

    fn refuse(vars: &HashMap<String, String>, fragment: &str) -> String {
        match guard_from_map(vars) {
            S3Guard::Refuse(reason) => {
                assert!(
                    reason.contains(fragment),
                    "refusal {reason:?} is missing {fragment:?}"
                );
                reason
            }
            S3Guard::Ready(_) => panic!("a bad fixture must not be accepted"),
            S3Guard::Skip => panic!("a bad fixture must not skip"),
        }
    }

    #[test]
    fn empty_settings_skip_before_any_io() {
        match guard_from_map(&HashMap::new()) {
            S3Guard::Skip => {}
            other => panic!(
                "empty settings must skip, got {}",
                match other {
                    S3Guard::Ready(_) => "ready",
                    S3Guard::Refuse(_) => "refuse",
                    S3Guard::Skip => "skip",
                }
            ),
        }
    }

    #[test]
    fn complete_settings_build_a_run_scoped_config() {
        let config = ready(&valid_vars());
        assert_eq!(config.s3_bucket, EXPECTED_BUCKET);
        assert_eq!(config.s3_region, "us-east-1");
        assert!(config.s3_prefix.starts_with("run-"));
        assert!(config.s3_allow_http);
        assert!(config.s3_path_style);
        assert!(matches!(config.backend, RawObjectStoreBackend::S3));
    }

    #[test]
    fn trailing_slashes_normalize_to_the_same_root() {
        let mut vars = valid_vars();
        vars.insert(
            ENDPOINT_KEY.to_string(),
            "http://s3-test.example.test:9002///".to_string(),
        );
        let config = ready(&vars);
        assert_eq!(config.s3_endpoint, "http://s3-test.example.test:9002");
    }

    #[test]
    fn partial_settings_fail_before_any_io() {
        let mut vars = valid_vars();
        vars.remove(SECRET_KEY_KEY);
        let reason = refuse(&vars, SECRET_KEY_KEY);
        assert!(
            !reason.contains("s3-fixture-secret"),
            "refusal must not echo values"
        );
    }

    #[test]
    fn another_bucket_is_refused() {
        let mut vars = valid_vars();
        vars.insert(BUCKET_KEY.to_string(), "someone-elses-bucket".to_string());
        refuse(&vars, BUCKET_KEY);
    }

    #[test]
    fn userinfo_query_and_fragment_are_refused() {
        for (label, endpoint, fragment) in [
            (
                "userinfo",
                "http://user@s3-test.example.test:9002",
                "userinfo",
            ),
            (
                "query",
                "http://s3-test.example.test:9002?x=1",
                "query parameters",
            ),
            (
                "fragment",
                "http://s3-test.example.test:9002#frag",
                "fragment",
            ),
            (
                "path",
                "http://s3-test.example.test:9002/minio",
                "service root",
            ),
        ] {
            let mut vars = valid_vars();
            vars.insert(ENDPOINT_KEY.to_string(), endpoint.to_string());
            let reason = refuse(&vars, fragment);
            assert!(
                !reason.contains("example.test"),
                "refusal for {label} must name the key, never the value"
            );
        }
    }

    #[test]
    fn unsupported_schemes_and_bad_ports_are_refused() {
        for endpoint in [
            "ftp://s3-test.example.test:9002",
            "http://:9002",
            "http://s3-test.example.test:notaport",
            "s3-test.example.test:9002",
        ] {
            let mut vars = valid_vars();
            vars.insert(ENDPOINT_KEY.to_string(), endpoint.to_string());
            refuse(&vars, ENDPOINT_KEY);
        }
    }

    #[test]
    fn loose_booleans_are_refused() {
        for (key, value) in [(ALLOW_HTTP_KEY, "yes"), (PATH_STYLE_KEY, "1")] {
            let mut vars = valid_vars();
            vars.insert(key.to_string(), value.to_string());
            refuse(&vars, "exactly true or false");
        }
    }

    #[test]
    fn plain_http_without_the_opt_in_is_refused() {
        let mut vars = valid_vars();
        vars.insert(ALLOW_HTTP_KEY.to_string(), "false".to_string());
        refuse(&vars, ALLOW_HTTP_KEY);
    }

    #[test]
    fn a_rewritten_confirmed_target_is_refused() {
        let mut vars = valid_vars();
        vars.insert(
            CONFIRMED_TARGET_KEY.to_string(),
            "http://s3-test.example.test:9002/other-bucket".to_string(),
        );
        refuse(&vars, CONFIRMED_TARGET_KEY);
    }

    #[test]
    fn refusals_name_keys_never_values() {
        let mut vars = valid_vars();
        vars.insert(BUCKET_KEY.to_string(), "wrong-bucket".to_string());
        let reason = refuse(&vars, BUCKET_KEY);
        assert!(!reason.contains("s3-fixture-secret"));
        assert!(!reason.contains("s3-fixture-access"));
        assert!(!reason.contains("wrong-bucket"));
    }
}
