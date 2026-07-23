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
}
