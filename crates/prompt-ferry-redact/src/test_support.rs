use std::sync::MutexGuard;

use crate::{RedactionConfig, TEST_REDACTION_LOCK, apply_config};
use redactor::RedactionRules;

pub fn lock() -> MutexGuard<'static, ()> {
    TEST_REDACTION_LOCK.lock().expect("test lock poisoned")
}

pub fn apply(config: &RedactionConfig) -> MutexGuard<'static, ()> {
    let guard = lock();
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
