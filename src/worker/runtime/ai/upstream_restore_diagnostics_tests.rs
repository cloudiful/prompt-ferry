//! Adjacent test module for `upstream_restore.rs` (#569 Phase 2): the restore
//! diagnostics must stay silent for clean results and warn exactly once with
//! the real bucket counts otherwise.

use redactor::RestoreResult;

use super::log_restore_diagnostics;

/// Captures `warn!` field values for the diagnostics assertions.
fn capture_warnings<R>(
    body: impl FnOnce() -> R,
) -> Vec<std::collections::BTreeMap<String, String>> {
    #[derive(Default)]
    struct Visitor {
        fields: std::collections::BTreeMap<String, String>,
    }
    impl tracing::field::Visit for Visitor {
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.fields
                .insert(field.name().to_string(), value.to_string());
        }
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.fields
                .insert(field.name().to_string(), format!("{value:?}"));
        }
    }
    let events: std::sync::Arc<std::sync::Mutex<Vec<std::collections::BTreeMap<String, String>>>> =
        std::sync::Arc::default();
    struct Collector {
        events: std::sync::Arc<std::sync::Mutex<Vec<std::collections::BTreeMap<String, String>>>>,
    }
    impl tracing::Subscriber for Collector {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let mut visitor = Visitor::default();
            event.record(&mut visitor);
            self.events
                .lock()
                .expect("capture lock")
                .push(visitor.fields);
        }
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }
    let collected = events.clone();
    tracing::subscriber::with_default(Collector { events }, body);
    collected.lock().expect("capture lock").clone()
}

fn result_with(skipped: usize, validation: usize, unresolved: usize) -> RestoreResult {
    RestoreResult {
        restored_text: String::new(),
        restored_count: 0,
        skipped_tokens: vec!["[[RDX:v2:...]]".to_string(); skipped],
        unresolved_tokens: vec!["[[RDX:v2:...]]".to_string(); unresolved],
        validation_errors: vec!["bad".to_string(); validation],
    }
}

#[test]
fn diagnostics_stay_silent_for_a_clean_restore() {
    let warnings = capture_warnings(|| {
        log_restore_diagnostics(&result_with(0, 0, 0), "ai_json");
    });
    assert!(
        warnings.is_empty(),
        "a clean restore must not warn: {warnings:?}"
    );
}

#[test]
fn diagnostics_warn_with_counts_when_any_bucket_is_non_empty() {
    for result in [
        result_with(1, 0, 0),
        result_with(0, 1, 0),
        result_with(0, 0, 1),
    ] {
        let warnings = capture_warnings(|| {
            log_restore_diagnostics(&result, "ai_json");
        });
        assert_eq!(warnings.len(), 1, "each dirty result warns once");
        assert_eq!(
            warnings[0].get("restore_surface").map(String::as_str),
            Some("ai_json")
        );
        assert_eq!(
            warnings[0].get("skipped_token_count"),
            Some(&result.skipped_tokens.len().to_string())
        );
        assert_eq!(
            warnings[0].get("validation_error_count"),
            Some(&result.validation_errors.len().to_string())
        );
        assert_eq!(
            warnings[0].get("unresolved_token_count"),
            Some(&result.unresolved_tokens.len().to_string())
        );
    }
}
