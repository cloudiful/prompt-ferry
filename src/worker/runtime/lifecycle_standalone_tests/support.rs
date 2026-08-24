//! Shared test helpers for the standalone request-lease slice. The
//! store- and lifecycle-level tests both need an isolated SQLite file
//! and a constructed `StandaloneRuntimeState`; centralising the
//! helpers here keeps the per-test files focused on assertions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::StandaloneConfigStore;
use crate::worker::runtime::standalone::StandaloneRuntimeState;

pub(crate) fn manager(byte: u8) -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([byte; 32])).expect("test manager")
}

pub(crate) fn database_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "prompt-ferry-standalone-request-leases-{}-{}-{}.sqlite",
        std::process::id(),
        suffix,
        counter
    ))
}

pub(crate) async fn open_store() -> (StandaloneConfigStore, PathBuf) {
    let path = database_path();
    let store = StandaloneConfigStore::open(&path)
        .await
        .expect("open standalone store");
    (store, path)
}

pub(crate) async fn cleanup(store: StandaloneConfigStore, path: PathBuf) {
    store.close().await;
    let _ = std::fs::remove_file(path);
}

pub(crate) async fn open_standalone_state() -> (StandaloneRuntimeState, PathBuf) {
    let path = database_path();
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let state = StandaloneRuntimeState::new(
        store,
        manager(7),
        crate::standalone_config::StandaloneConfig::default(),
    );
    (state, path)
}
