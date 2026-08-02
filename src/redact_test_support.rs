use std::sync::MutexGuard;

use crate::redact::{RedactionConfig, TEST_REDACTION_LOCK, apply_config};
use redactor::RedactionRules;

pub(crate) fn lock() -> MutexGuard<'static, ()> {
    TEST_REDACTION_LOCK.lock().expect("test lock poisoned")
}

pub(crate) fn apply(config: &RedactionConfig) -> MutexGuard<'static, ()> {
    let guard = lock();
    apply_config(config).expect("redaction config should apply");
    guard
}

pub(crate) fn domain_redaction() -> MutexGuard<'static, ()> {
    apply(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
}

pub(crate) fn secret_redaction() -> MutexGuard<'static, ()> {
    apply(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            secret: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
}
