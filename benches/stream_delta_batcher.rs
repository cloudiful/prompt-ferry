use std::time::{Duration, Instant};

use prompt_ferry::{
    db::StreamDeltaBatchingSettings, worker::stream_delta_batcher::StreamDeltaBatcher,
};
use serde_json::json;

fn sse_event(value: serde_json::Value) -> Vec<u8> {
    format!("data: {}\n\n", serde_json::to_string(&value).unwrap()).into_bytes()
}

fn sample_delta(index: usize) -> Vec<u8> {
    sse_event(json!({
        "type": "response.output_text.delta",
        "item_id": "msg_1",
        "output_index": 0,
        "content_index": 0,
        "delta": format!("chunk-{index}"),
    }))
}

fn bench_case(label: &str, settings: StreamDeltaBatchingSettings, iterations: usize) {
    let chunks = (0..iterations).map(sample_delta).collect::<Vec<_>>();
    let started_at = Instant::now();
    let mut emitted = 0usize;
    for _ in 0..100 {
        let mut batcher = StreamDeltaBatcher::new(settings.clone());
        for chunk in &chunks {
            emitted += batcher.push_chunk(chunk.clone()).unwrap().len();
        }
        emitted += batcher.finish().unwrap().len();
    }
    let elapsed = started_at.elapsed();
    println!(
        "{label}: {} iters in {:?} ({:.2} ns/chunk), emitted={emitted}",
        iterations * 100,
        elapsed,
        elapsed.as_nanos() as f64 / (iterations as f64 * 100.0),
    );
}

fn bench_multi_key_interleaved() {
    let settings = StreamDeltaBatchingSettings {
        enabled: true,
        flush_window_ms: 50,
        max_buffer_chars: 160,
        max_buffer_bytes: 1024,
        flush_on_line_break: true,
        flush_on_sentence_end: false,
    };
    // 4 keys, 128 chunks per key, interleave so each push lands on a key
    // different from the previous one; every round finishes cleanly.
    let chunks: Vec<Vec<u8>> = (0..128)
        .flat_map(|i| {
            (0..4).map(move |k| {
                sse_event(json!({
                    "type": "response.output_text.delta",
                    "item_id": format!("msg_{k}"),
                    "output_index": 0,
                    "content_index": 0,
                    "delta": format!("chunk-{i}"),
                    "sequence_number": (i * 4 + k) as i64,
                }))
            })
        })
        .collect();
    let rounds = 100usize;
    let started_at = Instant::now();
    let mut emitted = 0usize;
    for _ in 0..rounds {
        let mut batcher = StreamDeltaBatcher::new(settings.clone());
        for chunk in &chunks {
            emitted += batcher.push_chunk(chunk.clone()).unwrap().len();
        }
        emitted += batcher.finish().unwrap().len();
    }
    let elapsed = started_at.elapsed();
    let total_chunks = chunks.len() * rounds;
    println!(
        "multi_key_interleaved: {total_chunks} iters in {elapsed:?} ({:.2} ns/chunk), emitted={emitted}",
        elapsed.as_nanos() as f64 / total_chunks as f64,
    );
}

fn bench_flush_due_only() {
    // Aggressive flush window so a tiny sleep is enough for the time-based
    // branch to fire. The bench measures the flush_due path under no
    // additional push pressure.
    let settings = StreamDeltaBatchingSettings {
        enabled: true,
        flush_window_ms: 1,
        max_buffer_chars: 160,
        max_buffer_bytes: 1024,
        flush_on_line_break: true,
        flush_on_sentence_end: false,
    };
    let chunk = sample_delta(0);
    let rounds = 100usize;
    let flush_ticks = 4usize;
    let started_at = Instant::now();
    let mut emitted = 0usize;
    for _ in 0..rounds {
        let mut batcher = StreamDeltaBatcher::new(settings.clone());
        emitted += batcher.push_chunk(chunk.clone()).unwrap().len();
        for _ in 0..flush_ticks {
            std::thread::sleep(Duration::from_millis(2));
            emitted += batcher.flush_due().unwrap().len();
        }
    }
    let elapsed = started_at.elapsed();
    println!(
        "flush_due_only: {rounds} rounds x {flush_ticks} ticks in {elapsed:?}, emitted={emitted}",
    );
}

fn main() {
    let disabled = StreamDeltaBatchingSettings::default();
    let enabled = StreamDeltaBatchingSettings {
        enabled: true,
        flush_window_ms: 50,
        max_buffer_chars: 160,
        max_buffer_bytes: 1024,
        flush_on_line_break: true,
        flush_on_sentence_end: false,
    };

    println!("warming up...");
    std::thread::sleep(Duration::from_millis(50));
    bench_case("disabled", disabled, 512);
    bench_case("enabled", enabled, 512);
    bench_multi_key_interleaved();
    bench_flush_due_only();
}
