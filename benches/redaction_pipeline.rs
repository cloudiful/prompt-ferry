use std::hint::black_box;
use std::time::Instant;

use prompt_ferry::{
    redact::{RedactionConfig, apply_config},
    redact_upstream::UpstreamRedactionProcessor,
};
use redactor::RedactionRules;

fn timed(label: &str, iterations: usize, mut run: impl FnMut()) {
    let started = Instant::now();
    for _ in 0..iterations {
        run();
    }
    let elapsed = started.elapsed();
    println!(
        "{label}: {iterations} iterations in {elapsed:?} ({:.2} us/iter)",
        elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64
    );
}

fn main() {
    apply_config(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
    .expect("redaction config");

    let fields = (0..512)
        .map(|index| format!("service-{index}.example.com"))
        .collect::<Vec<_>>();
    timed("prompt_ferry_session_128_fields", 100, || {
        let mut processor = UpstreamRedactionProcessor::new(None, Some("bench"), None)
            .expect("redaction processor");
        let redacted = fields
            .iter()
            .take(128)
            .map(|field| {
                processor
                    .redact_fragment(field, redactor::InputKind::Text)
                    .expect("redact")
            })
            .collect::<Vec<_>>();
        black_box(
            processor
                .finish_state(&fields[..128].join("\n"), &redacted.join("\n"))
                .expect("finish state"),
        );
    });

    let mut processor =
        UpstreamRedactionProcessor::new(None, Some("bench"), None).expect("redaction processor");
    let redacted = fields
        .iter()
        .map(|field| {
            processor
                .redact_fragment(field, redactor::InputKind::Text)
                .expect("redact")
        })
        .collect::<Vec<_>>();
    let session = processor
        .finish_state(&fields.join("\n"), &redacted.join("\n"))
        .expect("finish state")
        .expect("session");
    timed("prompt_ferry_restore_512_entries_128_fields", 100, || {
        let context = session
            .restore_state
            .restore_context()
            .expect("restore context");
        for field in redacted.iter().take(128) {
            black_box(context.restore_text(black_box(field)));
        }
    });
}
