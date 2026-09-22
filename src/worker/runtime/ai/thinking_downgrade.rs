//! Issue #556: one-turn thinking downgrade for reasoning-echo rejections.
//!
//! A thinking-mode upstream rejects a tool-bearing request when the parent
//! assistant tool-call turn has no reasoning to pass back (`reasoning_content`
//! / `reasoning_text` in the thinking mode must be passed back). The rejection
//! can only happen on a turn that asks for thinking, carries tools, and has
//! nothing passable in the conversation history, so ferry turns thinking off
//! for exactly that turn:
//!
//! - pre-flight ([`resolve_thinking_disposition`]): the parent artifact chain
//!   proves the parent turn produced no reasoning and the outbound body
//!   carries nothing the upstream can consume. The `teo` per-target effort is
//!   already applied at this point, so the rewrite deliberately overrides it.
//!   Issue #562 gates this path on
//!   [`db::EndpointProvider::requires_reasoning_echo`]: only upstreams that
//!   actually reject a missing echo (DeepSeek) are downgraded, every other
//!   upstream keeps the requested thinking and relies on the retry below.
//! - retry (`forward`): the upstream still rejected the turn with the
//!   fingerprint, so the same request is resent once with thinking off. This
//!   path stays provider-independent and covers every upstream.
//!
//! Issue #566 makes the whole adaptation opt-in per target: both paths are
//! additionally gated on [`db::RouteConfig::thinking_downgrade_enabled`], and
//! a target that never opted in keeps the pre-#556 byte-identical passthrough.
//!
//! The rewrite never touches persistence, never fabricates reasoning, and
//! leaves `teo` target configuration alone. Setting
//! `PROMPT_FERRY_DISABLE_THINKING_DOWNGRADE=1` bypasses both paths and keeps
//! the forwarded bytes identical to the pre-#556 behavior, regardless of the
//! per-target switch.

use crate::{config::NativeApi, db, worker_admin::AdminState};
use serde_json::{Map, Value, json};
use tracing::warn;

/// Env escape hatch: `1` disables both the pre-flight downgrade and the
/// fingerprint retry.
pub(super) const DISABLE_ENV: &str = "PROMPT_FERRY_DISABLE_THINKING_DOWNGRADE";

const CHAT_DISABLED_THINKING: &str = "disabled";
const FERRY_REASONING_ECHO_PREFIXES: [&str; 2] = ["minimax-", "ferry-"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThinkingDisposition {
    AsRequested,
    DowngradedNoReasoning,
}

impl ThinkingDisposition {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::AsRequested => "as_requested",
            Self::DowngradedNoReasoning => "downgraded_no_reasoning",
        }
    }

    pub(super) fn is_downgraded(self) -> bool {
        matches!(self, Self::DowngradedNoReasoning)
    }
}

/// Whether the escape hatch is set. Checked before any body parsing so a
/// bypassed deployment forwards byte-identical requests.
pub(super) fn thinking_downgrade_bypassed() -> bool {
    std::env::var(DISABLE_ENV).as_deref() == Ok("1")
}

/// Whether the per-target switch enables the adaptation for one route. The
/// env escape hatch keeps the highest priority, so this only reports the
/// configured intent; callers pair it with [`thinking_downgrade_bypassed`].
pub(super) fn thinking_downgrade_enabled_for(route: &db::RouteConfig) -> bool {
    route.thinking_downgrade_enabled
}

/// The single downgrade rule: no parent reasoning, no restorable echo, a
/// thinking request, tools, and an upstream that requires the reasoning echo
/// are all required before ferry yields for one turn. Every other turn keeps
/// its requested thinking.
///
/// Issue #562 added `upstream_requires_echo`: a Responses upstream may meet the
/// four body/artifact conditions and still reject the rewrite (opencode go
/// refuses `reasoning.effort = "none"` with a hard 400), so the pre-flight
/// downgrade only fires for the DeepSeek-class upstreams that genuinely demand
/// the echo. The fingerprint retry stays provider-independent.
pub(super) fn decide_thinking(
    parent_has_reasoning: bool,
    echo_restorable: bool,
    thinking_requested: bool,
    has_tools: bool,
    upstream_requires_echo: bool,
    downgrade_enabled: bool,
) -> ThinkingDisposition {
    if downgrade_enabled
        && thinking_requested
        && has_tools
        && !parent_has_reasoning
        && !echo_restorable
        && upstream_requires_echo
    {
        ThinkingDisposition::DowngradedNoReasoning
    } else {
        ThinkingDisposition::AsRequested
    }
}

/// Body-level facts the decision needs, read from the final outbound body
/// (after translation and after the `teo` effort override).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct BodyThinkingSignals {
    pub(super) thinking_requested: bool,
    pub(super) has_tools: bool,
    /// The upstream can consume reasoning already present in the body:
    /// plaintext reasoning content, or a ferry-minted echo ferry restores
    /// before forwarding. Opaque provider blobs and summary-only reasoning
    /// items are not restorable.
    pub(super) reasoning_passable: bool,
}

pub(super) fn body_thinking_signals(
    provider: db::EndpointProvider,
    native_api: NativeApi,
    body: &[u8],
) -> BodyThinkingSignals {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return BodyThinkingSignals::default();
    };
    let Some(object) = value.as_object() else {
        return BodyThinkingSignals::default();
    };
    match native_api {
        NativeApi::Chat => BodyThinkingSignals {
            thinking_requested: chat_requests_thinking(provider, object),
            has_tools: has_tools(object),
            reasoning_passable: chat_reasoning_passable(object),
        },
        NativeApi::Responses => BodyThinkingSignals {
            thinking_requested: responses_requests_thinking(object),
            has_tools: has_tools(object),
            reasoning_passable: responses_reasoning_passable(provider, object),
        },
        NativeApi::Auto | NativeApi::AnthropicMessages | NativeApi::Realtime => {
            BodyThinkingSignals::default()
        }
    }
}

/// Resolve the disposition for one outbound request. The parent artifact
/// lookup only runs when the body-level conditions could end in a downgrade;
/// an unavailable lookup (no admin state, no parent event, missing artifact
/// row, or a failed query) is treated as "parent had reasoning" so an unknown
/// parent never changes forwarding behavior.
///
/// Issue #562: the pre-flight downgrade is gated on the route provider
/// requiring the reasoning echo, so a non-echo upstream (e.g. opencode go)
/// never receives a rewritten body and the parent lookup is skipped entirely.
///
/// Issue #566: the whole adaptation is additionally gated per target on
/// [`db::RouteConfig::thinking_downgrade_enabled`], so a target that never
/// opted in keeps the pre-#556 byte-identical passthrough (no rewrite and no
/// fingerprint retry) while the `PROMPT_FERRY_DISABLE_THINKING_DOWNGRADE=1`
/// escape hatch keeps the highest priority.
pub(super) async fn resolve_thinking_disposition(
    admin_state: Option<&AdminState>,
    parent_event_id: Option<i64>,
    route: &db::RouteConfig,
    body: &[u8],
) -> ThinkingDisposition {
    if thinking_downgrade_bypassed() {
        return ThinkingDisposition::AsRequested;
    }
    let downgrade_enabled = route.thinking_downgrade_enabled;
    let upstream_requires_echo = downgrade_enabled && route.provider.requires_reasoning_echo();
    let signals = body_thinking_signals(route.provider, route.native_api, body);
    let parent_has_reasoning = if upstream_requires_echo
        && signals.thinking_requested
        && signals.has_tools
        && !signals.reasoning_passable
    {
        parent_reasoning_present(admin_state, parent_event_id).await
    } else {
        true
    };
    decide_thinking(
        parent_has_reasoning,
        signals.reasoning_passable,
        signals.thinking_requested,
        signals.has_tools,
        upstream_requires_echo,
        downgrade_enabled,
    )
}

async fn parent_reasoning_present(
    admin_state: Option<&AdminState>,
    parent_event_id: Option<i64>,
) -> bool {
    let (Some(state), Some(parent_event_id)) = (admin_state, parent_event_id) else {
        return true;
    };
    match db::get_usage_assistant_artifacts(&state.pool, &[parent_event_id]).await {
        Ok(artifacts) => artifacts
            .iter()
            .find(|artifact| artifact.event_id == parent_event_id)
            .map(|artifact| artifact.has_reasoning_content)
            .unwrap_or(true),
        Err(error) => {
            warn!(
                error = %error,
                parent_event_id,
                "failed to load parent assistant artifact for thinking downgrade"
            );
            true
        }
    }
}

/// Turn thinking off for one outbound request body.
///
/// Chat bodies write `thinking: {"type":"disabled"}` and drop
/// `reasoning_effort` (which is what makes the rewrite bypass a `teo` effort
/// override); Responses bodies write `reasoning.effort = "none"`. Any other
/// protocol, non-JSON body, and non-object body are returned unchanged.
pub(super) fn apply_thinking_off(native_api: NativeApi, body: Vec<u8>) -> Vec<u8> {
    if !matches!(native_api, NativeApi::Chat | NativeApi::Responses) {
        return body;
    }
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(object) = value.as_object_mut() else {
        return body;
    };
    match native_api {
        NativeApi::Chat => {
            let already_off = object
                .get("thinking")
                .and_then(Value::as_object)
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str)
                == Some(CHAT_DISABLED_THINKING);
            if already_off && !object.contains_key("reasoning_effort") {
                return body;
            }
            object.remove("reasoning_effort");
            object.insert(
                "thinking".to_string(),
                json!({ "type": CHAT_DISABLED_THINKING }),
            );
        }
        NativeApi::Responses => {
            let reasoning = object.entry("reasoning").or_insert_with(|| json!({}));
            if let Some(reasoning_object) = reasoning.as_object_mut() {
                reasoning_object.insert("effort".to_string(), Value::String("none".to_string()));
            }
        }
        NativeApi::Auto | NativeApi::AnthropicMessages | NativeApi::Realtime => return body,
    }
    serde_json::to_vec(&value).unwrap_or(body)
}

fn chat_requests_thinking(provider: db::EndpointProvider, object: &Map<String, Value>) -> bool {
    if let Some(thinking) = object.get("thinking") {
        // Any caller-supplied thinking config that is not an explicit
        // `disabled` keeps thinking on (`enabled`, unknown objects, scalars).
        return thinking
            .as_object()
            .and_then(|thinking| thinking.get("type"))
            .and_then(Value::as_str)
            != Some(CHAT_DISABLED_THINKING);
    }
    if let Some(effort) = object.get("reasoning_effort").and_then(Value::as_str) {
        return !effort.trim().is_empty() && !effort.trim().eq_ignore_ascii_case("none");
    }
    // DeepSeek Chat enables thinking upstream by default and
    // `apply_deepseek_thinking` only makes that explicit, so an omitted
    // `thinking` field is a thinking request on that provider.
    provider == db::EndpointProvider::DeepSeek
}

fn responses_requests_thinking(object: &Map<String, Value>) -> bool {
    object
        .get("reasoning")
        .and_then(Value::as_object)
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|effort| !effort.is_empty() && !effort.eq_ignore_ascii_case("none"))
}

fn has_tools(object: &Map<String, Value>) -> bool {
    object
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty())
}

fn chat_reasoning_passable(object: &Map<String, Value>) -> bool {
    object
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            messages.iter().any(|message| {
                !crate::openai_compat::extract_text(
                    message.get("reasoning_content").unwrap_or(&Value::Null),
                )
                .trim()
                .is_empty()
            })
        })
}

fn responses_reasoning_passable(
    provider: db::EndpointProvider,
    object: &Map<String, Value>,
) -> bool {
    // `restore_reasoning_echoes` only runs on the outbound Responses body for
    // MiniMax targets; every other Responses target forwards the encrypted
    // blob verbatim, and a blob ferry cannot restore is not proof that the
    // upstream can consume the parent reasoning.
    let ferry_echo_restorable = provider == db::EndpointProvider::Minimax;
    object
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|input| {
            input
                .iter()
                .any(|item| reasoning_item_passable(item, ferry_echo_restorable))
        })
}

fn reasoning_item_passable(item: &Value, ferry_echo_restorable: bool) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("reasoning") {
        return false;
    }
    if has_reasoning_text(item) {
        return true;
    }
    ferry_echo_restorable
        && item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_some_and(is_ferry_reasoning_echo)
}

fn has_reasoning_text(item: &Value) -> bool {
    item.get("content")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts.iter().any(|part| {
                part.get("type").and_then(Value::as_str) == Some("reasoning_text")
                    && !crate::openai_compat::extract_text(part).trim().is_empty()
            })
        })
}

fn is_ferry_reasoning_echo(token: &str) -> bool {
    FERRY_REASONING_ECHO_PREFIXES
        .iter()
        .any(|prefix| token.strip_prefix(prefix).is_some_and(|id| !id.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::{
        DISABLE_ENV, ThinkingDisposition, apply_thinking_off, body_thinking_signals,
        decide_thinking, thinking_downgrade_bypassed,
    };
    use crate::{config::NativeApi, db};
    use serde_json::{Value, json};

    fn read_signals(
        provider: db::EndpointProvider,
        native_api: NativeApi,
        body: &Value,
    ) -> super::BodyThinkingSignals {
        body_thinking_signals(provider, native_api, &serde_json::to_vec(body).unwrap())
    }

    #[test]
    fn downgrades_only_when_nothing_can_be_passed_back() {
        assert_eq!(
            decide_thinking(false, false, true, true, true, true),
            ThinkingDisposition::DowngradedNoReasoning
        );
        // Parent reasoning present: the replayed turn can pass it back.
        assert_eq!(
            decide_thinking(true, false, true, true, true, true),
            ThinkingDisposition::AsRequested
        );
        // A restorable echo supplies the reasoning.
        assert_eq!(
            decide_thinking(false, true, true, true, true, true),
            ThinkingDisposition::AsRequested
        );
        // No tools: the upstream never asks for the tool-call reasoning.
        assert_eq!(
            decide_thinking(false, false, true, false, true, true),
            ThinkingDisposition::AsRequested
        );
        // Thinking already off: nothing to downgrade.
        assert_eq!(
            decide_thinking(false, false, false, true, true, true),
            ThinkingDisposition::AsRequested
        );
    }

    #[test]
    fn per_target_switch_off_never_downgrades() {
        // Issue #566: every other condition holds, but a target that never
        // opted in keeps the requested thinking (pre-#556 passthrough).
        assert_eq!(
            decide_thinking(false, false, true, true, true, false),
            ThinkingDisposition::AsRequested
        );
        // The switch is the only difference: the same inputs with it on
        // downgrade.
        assert_eq!(
            decide_thinking(false, false, true, true, true, true),
            ThinkingDisposition::DowngradedNoReasoning
        );
    }

    #[test]
    fn non_echo_upstream_never_pre_downgrades() {
        // Issue #562: all four body/artifact conditions hold, but the upstream
        // does not require the reasoning echo (opencode go and friends), so the
        // requested thinking is forwarded untouched and the fingerprint retry
        // remains the only fallback.
        assert_eq!(
            decide_thinking(false, false, true, true, false, true),
            ThinkingDisposition::AsRequested
        );
        // The gate is the only difference: same inputs with an echo upstream
        // still downgrade.
        assert_eq!(
            decide_thinking(false, false, true, true, true, true),
            ThinkingDisposition::DowngradedNoReasoning
        );
    }

    #[test]
    fn deepseek_chat_defaults_to_a_thinking_request() {
        let body = json!({"model": "m", "tools": [{"type": "function"}], "messages": []});
        let signals = read_signals(db::EndpointProvider::DeepSeek, NativeApi::Chat, &body);
        assert!(signals.thinking_requested);
        assert!(signals.has_tools);

        let disabled = json!({
            "model": "m",
            "thinking": {"type": "disabled"},
            "tools": [{"type": "function"}],
        });
        assert!(
            !read_signals(db::EndpointProvider::DeepSeek, NativeApi::Chat, &disabled)
                .thinking_requested
        );

        let none_effort = json!({"model": "m", "reasoning_effort": "none"});
        assert!(
            !read_signals(db::EndpointProvider::Generic, NativeApi::Chat, &none_effort)
                .thinking_requested
        );

        let effort = json!({"model": "m", "reasoning_effort": "high"});
        assert!(
            read_signals(db::EndpointProvider::Generic, NativeApi::Chat, &effort)
                .thinking_requested
        );

        // Non-DeepSeek chat without any thinking field stays as-is.
        let plain = json!({"model": "m", "messages": []});
        assert!(
            !read_signals(db::EndpointProvider::Generic, NativeApi::Chat, &plain)
                .thinking_requested
        );
    }

    #[test]
    fn chat_reasoning_content_counts_as_passable() {
        let with_reasoning = json!({
            "model": "m",
            "messages": [
                {"role": "assistant", "content": null, "reasoning_content": "plan"},
            ],
        });
        assert!(
            read_signals(
                db::EndpointProvider::DeepSeek,
                NativeApi::Chat,
                &with_reasoning
            )
            .reasoning_passable
        );

        let blank_reasoning = json!({
            "model": "m",
            "messages": [{"role": "assistant", "content": null, "reasoning_content": "  "}],
        });
        assert!(
            !read_signals(
                db::EndpointProvider::DeepSeek,
                NativeApi::Chat,
                &blank_reasoning
            )
            .reasoning_passable
        );
    }

    #[test]
    fn responses_reasoning_text_and_ferry_echo_are_passable() {
        let text = json!({
            "model": "m",
            "input": [
                {"type": "reasoning", "content": [{"type": "reasoning_text", "text": "plan"}]},
                {"type": "function_call", "call_id": "c1"},
            ],
        });
        assert!(
            read_signals(db::EndpointProvider::Generic, NativeApi::Responses, &text)
                .reasoning_passable
        );

        let echo = json!({
            "model": "m",
            "input": [{"type": "reasoning", "encrypted_content": "minimax-rs_1", "summary": []}],
        });
        assert!(
            read_signals(db::EndpointProvider::Minimax, NativeApi::Responses, &echo)
                .reasoning_passable
        );

        // The ferry echo is only restored for MiniMax Responses targets.
        assert!(
            !read_signals(db::EndpointProvider::Generic, NativeApi::Responses, &echo)
                .reasoning_passable
        );
    }

    #[test]
    fn opaque_echo_and_summary_only_items_are_not_restorable() {
        let opaque = json!({
            "model": "m",
            "input": [
                {"type": "reasoning", "encrypted_content": "6e4bd8b4-b70d-4f22", "summary": []},
            ],
        });
        let signals = read_signals(db::EndpointProvider::Minimax, NativeApi::Responses, &opaque);
        assert!(!signals.reasoning_passable);
        // Not restorable + thinking + tools + no parent reasoning = downgrade
        // on an echo-requiring upstream with the switch on.
        assert_eq!(
            decide_thinking(false, signals.reasoning_passable, true, true, true, true),
            ThinkingDisposition::DowngradedNoReasoning
        );
        // Issue #566: the same body on a target that never opted in stays
        // untouched.
        assert_eq!(
            decide_thinking(false, signals.reasoning_passable, true, true, true, false),
            ThinkingDisposition::AsRequested
        );

        let summary_only = json!({
            "model": "m",
            "input": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "plan"}]},
            ],
        });
        assert!(
            !read_signals(
                db::EndpointProvider::Minimax,
                NativeApi::Responses,
                &summary_only
            )
            .reasoning_passable
        );
    }

    #[test]
    fn responses_thinking_comes_from_an_explicit_effort() {
        let high = json!({"model": "m", "reasoning": {"effort": "high"}, "tools": [{}]});
        let signals = read_signals(db::EndpointProvider::Generic, NativeApi::Responses, &high);
        assert!(signals.thinking_requested);
        assert!(signals.has_tools);

        let none = json!({"model": "m", "reasoning": {"effort": "none"}});
        assert!(
            !read_signals(db::EndpointProvider::Generic, NativeApi::Responses, &none)
                .thinking_requested
        );

        let summary = json!({"model": "m", "reasoning": {"summary": "auto"}});
        assert!(
            !read_signals(
                db::EndpointProvider::Generic,
                NativeApi::Responses,
                &summary
            )
            .thinking_requested
        );
    }

    #[test]
    fn apply_thinking_off_rewrites_both_protocols() {
        let chat = serde_json::to_vec(&json!({
            "model": "m",
            "reasoning_effort": "high",
            "messages": [],
        }))
        .unwrap();
        let rewritten: Value =
            serde_json::from_slice(&apply_thinking_off(NativeApi::Chat, chat)).unwrap();
        assert_eq!(rewritten["thinking"]["type"], "disabled");
        assert!(rewritten.get("reasoning_effort").is_none());
        assert_eq!(rewritten["messages"], json!([]));

        let already_off =
            serde_json::to_vec(&json!({"model": "m", "thinking": {"type": "disabled"}})).unwrap();
        assert_eq!(
            apply_thinking_off(NativeApi::Chat, already_off.clone()),
            already_off
        );

        let responses =
            serde_json::to_vec(&json!({"model": "m", "reasoning": {"effort": "high"}})).unwrap();
        let rewritten: Value =
            serde_json::from_slice(&apply_thinking_off(NativeApi::Responses, responses)).unwrap();
        assert_eq!(rewritten["reasoning"]["effort"], "none");

        let responses_without_reasoning = serde_json::to_vec(&json!({"model": "m"})).unwrap();
        let rewritten: Value = serde_json::from_slice(&apply_thinking_off(
            NativeApi::Responses,
            responses_without_reasoning,
        ))
        .unwrap();
        assert_eq!(rewritten["reasoning"]["effort"], "none");
    }

    #[test]
    fn apply_thinking_off_leaves_other_protocols_and_bodies_untouched() {
        let anthropic = br#"{"model":"m","thinking":{"type":"enabled"}}"#.to_vec();
        assert_eq!(
            apply_thinking_off(NativeApi::AnthropicMessages, anthropic.clone()),
            anthropic
        );
        let not_json = b"not json".to_vec();
        assert_eq!(
            apply_thinking_off(NativeApi::Chat, not_json.clone()),
            not_json
        );
        let not_object = b"[]".to_vec();
        assert_eq!(
            apply_thinking_off(NativeApi::Responses, not_object.clone()),
            not_object
        );
    }

    #[test]
    fn env_escape_hatch_is_exact() {
        let isolation = EnvIsolation::new();
        unsafe { std::env::remove_var(DISABLE_ENV) };
        assert!(!thinking_downgrade_bypassed());
        unsafe { std::env::set_var(DISABLE_ENV, "0") };
        assert!(!thinking_downgrade_bypassed());
        unsafe { std::env::set_var(DISABLE_ENV, "1") };
        assert!(thinking_downgrade_bypassed());
        drop(isolation);
    }

    struct EnvIsolation;

    impl EnvIsolation {
        fn new() -> Self {
            unsafe { std::env::remove_var(DISABLE_ENV) };
            Self
        }
    }

    impl Drop for EnvIsolation {
        fn drop(&mut self) {
            unsafe { std::env::remove_var(DISABLE_ENV) };
        }
    }
}
