//! `#569` Phase 3: per-turn redaction cost across the context x rule/secret
//! matrix. One iteration is one conversation turn: a request whose context is
//! re-sent in full and re-scanned against a prior session that already mapped
//! its first half. The measured curve is the cost of the "#564 per-round full
//! rescan" the redaction chain still pays today.

mod redaction_support;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use redaction_support::{
    all_scenarios, apply_scenario, build_context, configure_group, redact_round, seeded_session,
};

fn redact_rounds(c: &mut Criterion) {
    let mut group = c.benchmark_group("redaction_round");
    for scenario in all_scenarios() {
        apply_scenario(scenario);
        let context = build_context(scenario);
        let prior = seeded_session(&context, "bench-round");
        configure_group(&mut group, scenario.context_chars);
        group.bench_with_input(
            BenchmarkId::new(scenario.function(), scenario.label()),
            &context,
            |bencher, context| {
                bencher.iter(|| {
                    black_box(redact_round(
                        black_box(context),
                        Some(&prior),
                        "bench-round",
                    ));
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, redact_rounds);
criterion_main!(benches);
