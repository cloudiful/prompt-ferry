mod runtime;
pub mod stream_delta_batcher;

pub use runtime::*;
// P4 (issue #230) re-exports the provider-aware URL composition helper
// so the model-route test probe and the Realtime WebSocket join stay
// single-sourced on the same function as the runtime HTTP path.
pub use runtime::upstream_url_for_route_parts;
