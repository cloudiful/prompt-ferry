//! `#569` Phase 3: per-round rescan budget for the upstream redaction chain.
//!
//! `#564` measured a full re-scan of the conversation on every turn, and the
//! optimization is a separate issue. Until it lands, this always-on `--lib`
//! test pins the current cost: one 100 KB turn (all built-in rules, 50 custom
//! secrets) must stay under [`BUDGET_P50_MS`] for its median. The threshold is
//! the release baseline measured in `benches/redaction_pipeline.rs` times a
//! debug-profile and shared-runner allowance; see `scripts/ci/README.md`.

use std::fmt::Write;
use std::time::{Duration, Instant};

use crate::redact::{RedactionConfig, RedactionCustomStringRule, apply_config};
use crate::redact_upstream::{UpstreamRedactionProcessor, UpstreamRedactionSession};
use redactor::{CustomStringMatch, CustomStringScope, InputKind, RedactionRules};

const CONTEXT_CHARS: usize = 100_000;
const SECRET_COUNT: usize = 50;
const WARMUP_ROUNDS: usize = 2;
const MEASURED_ROUNDS: usize = 15;
/// Debug-profile upper bound for the 100 KB median turn; see module docs.
const BUDGET_P50_MS: u64 = 1_500;

#[test]
fn per_round_rescan_stays_within_budget_at_100kb() {
    let _guard = crate::redact::test_support::lock();
    apply_config(&config()).expect("apply budget redaction config");

    let context = build_context();
    let (redacted_half, prior) = redact_round(&half(&context), None);
    assert!(
        redacted_half.contains("[[RDX:v2:"),
        "budget scenario must actually redact"
    );

    for _ in 0..WARMUP_ROUNDS {
        std::hint::black_box(redact_round(&context, Some(&prior)));
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(MEASURED_ROUNDS);
    for _ in 0..MEASURED_ROUNDS {
        let started = Instant::now();
        let (redacted, session) = redact_round(&context, Some(&prior));
        samples.push(started.elapsed());
        assert!(redacted.contains("[[RDX:v2:"), "round must keep redacting");
        std::hint::black_box(session);
    }
    samples.sort_unstable();
    let p50 = samples[samples.len() / 2];
    eprintln!(
        "per-round rescan budget: 100 KB p50 = {:.1} ms (budget {BUDGET_P50_MS} ms)",
        p50.as_secs_f64() * 1000.0
    );
    assert!(
        p50 < Duration::from_millis(BUDGET_P50_MS),
        "100 KB per-round rescan p50 {p50:?} exceeded the {BUDGET_P50_MS} ms budget"
    );
}

fn config() -> RedactionConfig {
    RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            secret: true,
            domain: true,
            url: true,
            email: true,
            ip: true,
            cidr: true,
            phone: true,
            person: true,
            organization: true,
        },
        custom_strings: (0..SECRET_COUNT)
            .map(|index| RedactionCustomStringRule {
                pattern: format!("secret-token-{index:04}"),
                match_type: CustomStringMatch::Contains,
                scope: CustomStringScope::Text,
                ..RedactionCustomStringRule::default()
            })
            .collect(),
    }
}

fn build_context() -> String {
    let mut text = String::with_capacity(CONTEXT_CHARS + 256);
    let mut index = 0usize;
    while text.len() < CONTEXT_CHARS {
        writeln!(
            text,
            "line {index:06}: host-{index}.example.com user-{index}@example.com secret secret-token-{:04} status ok",
            index % SECRET_COUNT
        )
        .expect("write budget context line");
        index += 1;
    }
    text.truncate(CONTEXT_CHARS);
    text
}

fn half(context: &str) -> String {
    context[..context.len() / 2].to_string()
}

fn redact_round(
    context: &str,
    prior: Option<&UpstreamRedactionSession>,
) -> (String, UpstreamRedactionSession) {
    let mut processor =
        UpstreamRedactionProcessor::new(None, Some("budget"), prior).expect("budget processor");
    let redacted = processor
        .redact_fragment(context, InputKind::Text)
        .expect("budget redact");
    let session = processor
        .finish_state(context, &redacted)
        .expect("budget finish")
        .expect("budget session");
    (redacted, session)
}
