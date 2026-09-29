//! `#569` Phase 3 shared scenario builders for the redaction criterion benches.
//!
//! The redact side replays one conversation turn over a context of 10k/100k/
//! 500k characters with a varying number of enabled built-in rules and custom
//! secret strings; the restore side replays one turn of token restoration over
//! the same context. Scenario ids carry a `quick`/`full` tier so CI can gate
//! only the bounded subset (`cargo bench ... -- quick`) and never the
//! full-load matrix. See `scripts/ci/README.md`.

#![allow(dead_code)]

use std::fmt::Write;
use std::time::Duration;

use criterion::{BenchmarkGroup, measurement::WallTime};
use prompt_ferry_redact::{RedactionConfig, RedactionCustomStringRule, apply_config};
use prompt_ferry_redact_upstream::{UpstreamRedactionProcessor, UpstreamRedactionSession};
use redactor::{CustomStringMatch, CustomStringScope, InputKind, RedactionRules};

/// The conversation sizes the `#569` Phase 3 matrix sweeps.
pub const CONTEXT_CHARS: [usize; 3] = [10_000, 100_000, 500_000];
/// The custom-string ("secret") counts the matrix sweeps.
pub const SECRET_COUNTS: [usize; 3] = [0, 50, 200];

/// The two built-in rule presets: the minimum redaction chain and every rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleSet {
    DomainOnly,
    AllRules,
}

impl RuleSet {
    pub const ALL: [RuleSet; 2] = [RuleSet::DomainOnly, RuleSet::AllRules];

    pub fn label(self) -> &'static str {
        match self {
            RuleSet::DomainOnly => "domain",
            RuleSet::AllRules => "all9",
        }
    }

    pub fn rules(self) -> RedactionRules {
        match self {
            RuleSet::DomainOnly => RedactionRules {
                domain: true,
                ..RedactionRules::default()
            },
            RuleSet::AllRules => RedactionRules {
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
        }
    }
}

/// One matrix cell: context size x enabled rule set x custom-string count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scenario {
    pub context_chars: usize,
    pub secret_count: usize,
    pub rule_set: RuleSet,
}

impl Scenario {
    /// The bounded subset CI gates: every rule enabled, at most 50 secrets, and
    /// contexts at or below 100k (100k only without secrets).
    pub fn quick(self) -> bool {
        if self.rule_set != RuleSet::AllRules {
            return false;
        }
        matches!(
            (self.context_chars, self.secret_count),
            (10_000, 0 | 50) | (100_000, 0)
        )
    }

    pub fn tier(self) -> &'static str {
        if self.quick() { "quick" } else { "full" }
    }

    fn context_label(self) -> String {
        format!("ctx_{}k", self.context_chars / 1000)
    }

    /// Criterion function id, e.g. `quick/ctx_10k`.
    pub fn function(self) -> String {
        format!("{}/{}", self.tier(), self.context_label())
    }

    /// Criterion parameter id, e.g. `rules_all9_secrets_50`.
    pub fn label(self) -> String {
        format!(
            "rules_{}_secrets_{}",
            self.rule_set.label(),
            self.secret_count
        )
    }
}

/// The full 3 x 2 x 3 matrix.
pub fn all_scenarios() -> Vec<Scenario> {
    let mut scenarios = Vec::new();
    for context_chars in CONTEXT_CHARS {
        for rule_set in RuleSet::ALL {
            for secret_count in SECRET_COUNTS {
                scenarios.push(Scenario {
                    context_chars,
                    secret_count,
                    rule_set,
                });
            }
        }
    }
    scenarios
}

pub fn secret_pattern(index: usize) -> String {
    format!("secret-token-{index:04}")
}

/// Install this scenario's policy as the process-global redaction config so
/// `UpstreamRedactionProcessor::new` picks it up.
pub fn apply_scenario(scenario: Scenario) {
    let custom_strings = (0..scenario.secret_count)
        .map(|index| RedactionCustomStringRule {
            pattern: secret_pattern(index),
            match_type: CustomStringMatch::Contains,
            scope: CustomStringScope::Text,
            ..RedactionCustomStringRule::default()
        })
        .collect();
    apply_config(&RedactionConfig {
        enabled: true,
        rules: scenario.rule_set.rules(),
        custom_strings,
    })
    .expect("apply redaction bench config");
}

/// Deterministic context of `scenario.context_chars` bytes. Every line carries
/// a unique host/user/address so the session accumulates one entry per line;
/// custom-string patterns cycle through the requested secret count.
pub fn build_context(scenario: Scenario) -> String {
    let mut text = String::with_capacity(scenario.context_chars + 256);
    let mut index = 0usize;
    while text.len() < scenario.context_chars {
        if scenario.secret_count == 0 {
            writeln!(
                text,
                "line {index:06}: host-{index}.example.com user-{index}@example.com addr 10.0.{}.{} status ok",
                index / 250,
                index % 250
            )
            .expect("write context line");
        } else {
            writeln!(
                text,
                "line {index:06}: host-{index}.example.com user-{index}@example.com secret {} status ok",
                secret_pattern(index % scenario.secret_count)
            )
            .expect("write context line");
        }
        index += 1;
    }
    truncate_on_char_boundary(text, scenario.context_chars)
}

/// Redact `context` as one turn, optionally continuing `prior`. Returning the
/// finished session keeps the multi-turn advance on the timed path.
pub fn redact_round(
    context: &str,
    prior: Option<&UpstreamRedactionSession>,
    external_id: &str,
) -> (String, UpstreamRedactionSession) {
    let mut processor =
        UpstreamRedactionProcessor::new(None, Some(external_id), prior).expect("processor");
    let redacted = processor
        .redact_fragment(context, InputKind::Text)
        .expect("redact round");
    let session = processor
        .finish_state(context, &redacted)
        .expect("finish round")
        .expect("round session");
    (redacted, session)
}

/// Prior session that already holds the first half of `context`, so a timed
/// round re-scans text whose earlier entities are already mapped.
pub fn seeded_session(context: &str, external_id: &str) -> UpstreamRedactionSession {
    let split = context
        .char_indices()
        .map(|(offset, _)| offset)
        .take_while(|offset| *offset <= context.len() / 2)
        .last()
        .unwrap_or(0);
    redact_round(&context[..split], None, external_id).1
}

/// Bound bench cost: default sampling for small contexts, the minimum sample
/// size plus a shorter measurement window for 100k/500k.
pub fn configure_group(group: &mut BenchmarkGroup<'_, WallTime>, context_chars: usize) {
    if context_chars >= 100_000 {
        group.sample_size(10);
        group.measurement_time(Duration::from_secs(3));
    } else {
        group.sample_size(30);
    }
}

fn truncate_on_char_boundary(mut text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}
