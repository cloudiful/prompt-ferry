use tokio::sync::MutexGuard;

use crate::{RedactionConfig, TEST_REDACTION_LOCK, apply_config};
use redactor::RedactionRules;

/// Sync-test-only helpers below (`lock`, `apply`, `domain_redaction`,
/// `secret_redaction`) use `blocking_lock` and must only be called from sync
/// `#[test]` tests. Async `#[tokio::test]` tests must use the `*_async`
/// variants, since `blocking_lock` panics inside an async runtime.
pub fn lock() -> MutexGuard<'static, ()> {
    TEST_REDACTION_LOCK.blocking_lock()
}

pub async fn lock_async() -> MutexGuard<'static, ()> {
    TEST_REDACTION_LOCK.lock().await
}

pub fn apply(config: &RedactionConfig) -> MutexGuard<'static, ()> {
    let guard = lock();
    apply_config(config).expect("redaction config should apply");
    guard
}

pub async fn apply_async(config: &RedactionConfig) -> MutexGuard<'static, ()> {
    let guard = lock_async().await;
    apply_config(config).expect("redaction config should apply");
    guard
}

pub fn domain_redaction() -> MutexGuard<'static, ()> {
    apply(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
}

pub async fn domain_redaction_async() -> MutexGuard<'static, ()> {
    apply_async(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
    .await
}

pub fn secret_redaction() -> MutexGuard<'static, ()> {
    apply(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            secret: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
}

pub async fn secret_redaction_async() -> MutexGuard<'static, ()> {
    apply_async(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            secret: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
    .await
}
