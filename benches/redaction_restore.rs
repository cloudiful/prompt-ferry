//! `#569` Phase 3: restore-side cost across the context x rule/secret matrix.
//! One iteration restores the redacted text of one fully re-sent conversation
//! turn, so the curve answers how restore-side token scanning grows with the
//! same context/secret dimensions the redact bench sweeps.

mod redaction_support;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use redaction_support::{
    all_scenarios, apply_scenario, build_context, configure_group, redact_round, seeded_session,
};

fn restore_rounds(c: &mut Criterion) {
    let mut group = c.benchmark_group("redaction_restore");
    for scenario in all_scenarios() {
        apply_scenario(scenario);
        let context = build_context(scenario);
        let prior = seeded_session(&context, "bench-restore");
        let (redacted, session) = redact_round(&context, Some(&prior), "bench-restore");
        let restore = session
            .restore_state
            .restore_context()
            .expect("restore context");
        configure_group(&mut group, scenario.context_chars);
        group.bench_with_input(
            BenchmarkId::new(scenario.function(), scenario.label()),
            &redacted,
            |bencher, redacted| {
                bencher.iter(|| black_box(restore.restore_text(black_box(redacted))));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, restore_rounds);
criterion_main!(benches);
