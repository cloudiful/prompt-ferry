//! Issue #757 P1: automatic prompt caching for OpenRouter Responses requests
//! that resolve to an Anthropic model.
//!
//! A targeted request gains the root `cache_control: {"type": "ephemeral"}`
//! directive (documented default TTL, so no `ttl` is written). Every other
//! request — other providers, protocols or prepared paths, malformed or
//! non-object bodies, bodies already carrying a cache marker — is returned as
//! the exact incoming bytes with its original `Cow` variant. Caching is
//! enabled here, not a guaranteed provider cache hit.

use std::borrow::Cow;

use serde_json::{Map, Value, json};

use crate::config::NativeApi;
use crate::db::{EndpointProvider, RouteConfig};

/// OpenRouter exposes automatic caching on the Responses path only; the caller
/// passes the actual prepared upstream path rather than a reconstructed one.
const CACHE_PATH: &str = "/v1/responses";
/// OpenRouter's vendor-prefixed model namespace. Case-sensitive, and a bare
/// prefix without a model suffix is not a model.
const ANTHROPIC_MODEL_PREFIX: &str = "anthropic/";
/// Root directive key written for a targeted request.
const CACHE_CONTROL_KEY: &str = "cache_control";
/// Keys that mean the caller already expressed cache intent, at any depth and
/// with any value (including `null`, `false`, or an invalid shape).
const CALLER_MARKER_KEYS: [&str; 2] = [CACHE_CONTROL_KEY, "prompt_cache_breakpoint"];
/// Upper bound on examined JSON nodes; a body that cannot be reviewed in full
/// is forwarded untouched.
const MAX_SCANNED_NODES: usize = 16_384;

#[cfg(test)]
mod tests;

/// Add the OpenRouter root cache directive to a targeted request. Every no-op
/// path returns `body` unchanged, `Cow` variant included.
pub(super) fn apply_openrouter_prompt_cache<'a>(
    route: &RouteConfig,
    path: &str,
    body: Cow<'a, [u8]>,
) -> Cow<'a, [u8]> {
    if !is_cache_target(route, path) {
        return body;
    }
    let Ok(value) = serde_json::from_slice::<Value>(body.as_ref()) else {
        return body;
    };
    let Some(object) = value.as_object() else {
        return body;
    };
    if !targets_anthropic_model(route, object) {
        return body;
    }
    if !is_free_of_caller_markers(&value) {
        return body;
    }
    let mut value = value;
    if let Some(object) = value.as_object_mut() {
        object.insert(CACHE_CONTROL_KEY.to_string(), ephemeral_directive());
    }
    // A serialization failure must not forward a half-rewritten body.
    match serde_json::to_vec(&value) {
        Ok(bytes) => Cow::Owned(bytes),
        Err(_) => body,
    }
}

fn is_cache_target(route: &RouteConfig, path: &str) -> bool {
    route.provider == EndpointProvider::OpenRouter
        && route.native_api == NativeApi::Responses
        && path == CACHE_PATH
}

/// A configured route model is authoritative: when it is not an Anthropic
/// model the caller's body model never re-enables the directive. Only a route
/// without a configured model falls back to the caller's `model` string.
fn targets_anthropic_model(route: &RouteConfig, object: &Map<String, Value>) -> bool {
    match route.upstream_model.as_deref() {
        Some(model) => is_anthropic_model(model),
        None => object
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(is_anthropic_model),
    }
}

fn is_anthropic_model(model: &str) -> bool {
    model
        .strip_prefix(ANTHROPIC_MODEL_PREFIX)
        .is_some_and(|suffix| !suffix.is_empty())
}

/// Iterative, depth-independent scan for caller cache intent. `false` also
/// covers the bounded fail-safe: a body with more nodes than the cap allows is
/// forwarded untouched rather than rewritten.
fn is_free_of_caller_markers(root: &Value) -> bool {
    let mut pending: Vec<&Value> = vec![root];
    let mut scanned = 0usize;
    while let Some(value) = pending.pop() {
        scanned += 1;
        if scanned > MAX_SCANNED_NODES {
            return false;
        }
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if CALLER_MARKER_KEYS.contains(&key.as_str()) {
                        return false;
                    }
                    pending.push(child);
                }
            }
            Value::Array(items) => pending.extend(items.iter()),
            _ => {}
        }
    }
    true
}

fn ephemeral_directive() -> Value {
    json!({"type": "ephemeral"})
}
