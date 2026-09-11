//! Official upstream base URLs for preset providers (issue #248).
//!
//! Preset providers (MiniMax, GLM, CommandCode, OpencodeGo, OpenRouter,
//! DeepSeek) no
//! longer expose a base URL in the admin form. The admin API derives the
//! base from `(provider, provider_region, native_api)` on create/update, and
//! the runtime, model discovery, quota and connectivity paths derive it
//! again so rows whose stored base was mangled by the legacy trailing-`/v1`
//! strip self-heal. Generic rows keep their operator-supplied base verbatim.
//!
//! [`preset_base_url`] is the single source of truth for the mapping.
//! [`derive_route_base`] adds the "official host" guard used by read paths:
//! a preset row whose stored base still points at an official (or known
//! mirror) host is re-derived, while a row on an unrelated host is treated
//! as a legacy/custom override and left untouched so existing deployments
//! and local upstream test doubles keep working.

use crate::config::NativeApi;
use crate::db::{EndpointProvider, EndpointRegion};

/// CommandCode provider-compatible inference root.
pub const COMMAND_CODE_BASE_URL: &str = "https://api.commandcode.ai/provider";
/// OpencodeGo Zen inference root (the `/v1` segment is appended by the joiner).
pub const OPENCODE_GO_BASE_URL: &str = "https://opencode.ai/zen/go";
/// OpenRouter inference root (the `/v1` segment is appended by the joiner).
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api";
/// DeepSeek inference root (the `/v1` segment is appended by the joiner).
pub const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
/// Zhipu Coding Plan Anthropic Messages root.
pub const GLM_ANTHROPIC_BASE_URL: &str = "https://open.bigmodel.cn/api/anthropic";
/// Zhipu Coding Plan Chat (and Realtime) root.
pub const GLM_CHAT_BASE_URL: &str = "https://open.bigmodel.cn/api/coding/paas/v4";
/// Zhipu Coding Plan Responses root.
pub const GLM_RESPONSES_BASE_URL: &str = "https://open.bigmodel.cn/api/v1";
/// MiniMax China inference root.
pub const MINIMAX_CN_BASE_URL: &str = "https://api.minimaxi.com";
/// MiniMax global inference root.
pub const MINIMAX_GLOBAL_BASE_URL: &str = "https://api.minimax.io";

/// Derive the official inference base for a preset provider.
///
/// `None` means the provider owns its base (Generic) or the derivation has
/// no answer (MiniMax without a region). GLM resolves per `native_api`:
/// Anthropic Messages keeps the standard `/v1/messages` path because the
/// Anthropic root does not carry a version segment, Responses uses the
/// `/api/v1` root, and Chat/Realtime share the `/api/coding/paas/v4` root.
///
/// `Auto` is resolved to a concrete protocol before derivation at every
/// request-time call site. When an unresolved `Auto` still reaches here
/// (Realtime WebSocket join / model-route probe) it falls back to the Chat
/// family root so the joined path can never be a wrong per-protocol guess.
pub fn preset_base_url(
    provider: EndpointProvider,
    region: Option<EndpointRegion>,
    native_api: NativeApi,
) -> Option<&'static str> {
    match provider {
        EndpointProvider::Generic => None,
        EndpointProvider::Minimax => match region {
            Some(EndpointRegion::Cn) => Some(MINIMAX_CN_BASE_URL),
            Some(EndpointRegion::Global) => Some(MINIMAX_GLOBAL_BASE_URL),
            None => None,
        },
        EndpointProvider::Glm => Some(match native_api {
            NativeApi::AnthropicMessages => GLM_ANTHROPIC_BASE_URL,
            NativeApi::Responses => GLM_RESPONSES_BASE_URL,
            NativeApi::Auto | NativeApi::Chat | NativeApi::Realtime => GLM_CHAT_BASE_URL,
        }),
        EndpointProvider::CommandCode => Some(COMMAND_CODE_BASE_URL),
        EndpointProvider::OpencodeGo => Some(OPENCODE_GO_BASE_URL),
        EndpointProvider::OpenRouter => Some(OPENROUTER_BASE_URL),
        EndpointProvider::DeepSeek => Some(DEEPSEEK_BASE_URL),
    }
}

/// Derive the read-path base for a stored row.
///
/// Returns `Some(derived)` only when the stored base still points at an
/// official (or known mirror) host for the provider; the stored path is
/// ignored so a mangled `/api`/`/v1` suffix is replaced by the official
/// root. Returns `None` for Generic and for preset rows on a custom host,
/// whose stored base the caller keeps verbatim.
pub fn derive_route_base(
    provider: EndpointProvider,
    stored_base: &str,
    native_api: NativeApi,
) -> Option<String> {
    let host = host_of(stored_base)?;
    if !host_is_preset(provider, &host) {
        return None;
    }
    let region = minimax_region_from_host(&host);
    preset_base_url(provider, region, native_api).map(str::to_string)
}

/// [`derive_route_base`] with the stored base as the fallback. Read paths
/// that need a concrete base string (model discovery, quota) use this so a
/// legacy/custom host keeps working instead of being dropped.
pub(crate) fn route_base_or_stored(
    provider: EndpointProvider,
    stored_base: &str,
    native_api: NativeApi,
) -> String {
    derive_route_base(provider, stored_base, native_api)
        .unwrap_or_else(|| stored_base.trim_end_matches('/').to_string())
}

/// True when `base_url` belongs to the provider's official host set.
fn host_is_preset(provider: EndpointProvider, host: &str) -> bool {
    match provider {
        EndpointProvider::Generic => false,
        EndpointProvider::Minimax => minimax_region_from_host(host).is_some(),
        EndpointProvider::Glm => matches!(host, "open.bigmodel.cn" | "api.z.ai"),
        EndpointProvider::CommandCode => host == "api.commandcode.ai",
        EndpointProvider::OpencodeGo => host == "opencode.ai" || host.ends_with(".opencode.ai"),
        EndpointProvider::OpenRouter => host == "openrouter.ai" || host.ends_with(".openrouter.ai"),
        EndpointProvider::DeepSeek => host == "api.deepseek.com" || host.ends_with(".deepseek.com"),
    }
}

fn minimax_region_from_host(host: &str) -> Option<EndpointRegion> {
    match host {
        "api.minimaxi.com" => Some(EndpointRegion::Cn),
        "api.minimax.io" => Some(EndpointRegion::Global),
        _ => None,
    }
}

/// Lowercase host of an http(s) URL, or `None` when unparseable/blank.
fn host_of(base_url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(base_url.trim()).ok()?;
    parsed.host_str().map(|host| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glm_derives_three_protocol_bases() {
        assert_eq!(
            preset_base_url(EndpointProvider::Glm, None, NativeApi::AnthropicMessages),
            Some(GLM_ANTHROPIC_BASE_URL)
        );
        assert_eq!(
            preset_base_url(EndpointProvider::Glm, None, NativeApi::Responses),
            Some(GLM_RESPONSES_BASE_URL)
        );
        for native_api in [NativeApi::Chat, NativeApi::Realtime, NativeApi::Auto] {
            assert_eq!(
                preset_base_url(EndpointProvider::Glm, None, native_api),
                Some(GLM_CHAT_BASE_URL),
                "GLM {native_api:?} must use the Chat family root"
            );
        }
    }

    #[test]
    fn minimax_honours_region_and_rejects_missing_region() {
        assert_eq!(
            preset_base_url(
                EndpointProvider::Minimax,
                Some(EndpointRegion::Cn),
                NativeApi::Chat
            ),
            Some(MINIMAX_CN_BASE_URL)
        );
        assert_eq!(
            preset_base_url(
                EndpointProvider::Minimax,
                Some(EndpointRegion::Global),
                NativeApi::AnthropicMessages
            ),
            Some(MINIMAX_GLOBAL_BASE_URL)
        );
        assert_eq!(
            preset_base_url(EndpointProvider::Minimax, None, NativeApi::Chat),
            None
        );
    }

    #[test]
    fn non_minimax_presets_use_fixed_official_bases() {
        for native_api in [
            NativeApi::AnthropicMessages,
            NativeApi::Chat,
            NativeApi::Responses,
            NativeApi::Realtime,
            NativeApi::Auto,
        ] {
            assert_eq!(
                preset_base_url(EndpointProvider::CommandCode, None, native_api),
                Some(COMMAND_CODE_BASE_URL)
            );
            assert_eq!(
                preset_base_url(EndpointProvider::OpencodeGo, None, native_api),
                Some(OPENCODE_GO_BASE_URL)
            );
            assert_eq!(
                preset_base_url(EndpointProvider::OpenRouter, None, native_api),
                Some(OPENROUTER_BASE_URL)
            );
            assert_eq!(
                preset_base_url(EndpointProvider::DeepSeek, None, native_api),
                Some(DEEPSEEK_BASE_URL)
            );
        }
    }

    #[test]
    fn generic_has_no_preset_base() {
        assert_eq!(
            preset_base_url(EndpointProvider::Generic, None, NativeApi::Chat),
            None
        );
    }

    #[test]
    fn derive_route_base_ignores_mangled_official_paths() {
        for base in [
            "https://open.bigmodel.cn/api",
            "https://open.bigmodel.cn/api/v1",
            "https://api.z.ai/api",
        ] {
            assert_eq!(
                derive_route_base(EndpointProvider::Glm, base, NativeApi::Responses).as_deref(),
                Some(GLM_RESPONSES_BASE_URL),
                "mangled GLM base {base} must self-heal"
            );
        }
        assert_eq!(
            derive_route_base(
                EndpointProvider::OpenRouter,
                "https://openrouter.ai/api/v1",
                NativeApi::Chat
            )
            .as_deref(),
            Some(OPENROUTER_BASE_URL)
        );
        assert_eq!(
            derive_route_base(
                EndpointProvider::CommandCode,
                "https://api.commandcode.ai/provider/v1",
                NativeApi::Chat
            )
            .as_deref(),
            Some(COMMAND_CODE_BASE_URL)
        );
        assert_eq!(
            derive_route_base(
                EndpointProvider::DeepSeek,
                "https://api.deepseek.com/v1",
                NativeApi::Chat
            )
            .as_deref(),
            Some(DEEPSEEK_BASE_URL)
        );
    }

    #[test]
    fn derive_route_base_recovers_minimax_region_from_host() {
        assert_eq!(
            derive_route_base(
                EndpointProvider::Minimax,
                "https://api.minimaxi.com/v1",
                NativeApi::Chat
            )
            .as_deref(),
            Some(MINIMAX_CN_BASE_URL)
        );
        assert_eq!(
            derive_route_base(
                EndpointProvider::Minimax,
                "https://api.minimax.io",
                NativeApi::Chat
            )
            .as_deref(),
            Some(MINIMAX_GLOBAL_BASE_URL)
        );
    }

    #[test]
    fn derive_route_base_leaves_generic_and_custom_hosts_untouched() {
        for (provider, base) in [
            (EndpointProvider::Generic, "https://api.openai.com/v1"),
            (EndpointProvider::Minimax, "https://proxy.example.test"),
            (EndpointProvider::Glm, "https://proxy.example.test"),
            (EndpointProvider::OpenRouter, "https://proxy.example.test"),
            (EndpointProvider::DeepSeek, "https://proxy.example.test"),
        ] {
            assert_eq!(
                derive_route_base(provider, base, NativeApi::Chat),
                None,
                "{provider:?} on {base} must keep the stored base"
            );
        }
    }
}
