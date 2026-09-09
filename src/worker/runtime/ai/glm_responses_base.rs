//! GLM Responses base validation (issue #238).
//!
//! The Zhipu Coding Plan serves three protocols on three independent
//! bases: Chat at `.../api/coding/paas/v4`, Anthropic at `.../api/anthropic`,
//! and Responses at `.../api/v1`. The runtime URL composer
//! ([`super::upstream::upstream_url_for_route_parts`]) strips the leading
//! `/v1` from Chat/Responses/Realtime paths so the joined URL is
//! `<base>/<path>`. When the operator points a Responses route at the
//! Chat family base, the join produces
//! `https://open.bigmodel.cn/api/coding/paas/v4/responses`, which the
//! Zhipu origin always 404s. This helper fails fast with a clear
//! remediation message instead of building the guaranteed-404 URL.
//!
//! Generic / MiniMax routes and the correct `.../api/v1` + Responses
//! join pass through unchanged; Anthropic/Chat joins on the same base
//! also pass through because the fail-fast is scoped to
//! `(Glm, Responses, base contains /coding/paas)`.

use http::StatusCode;

use crate::{config::NativeApi, db::EndpointProvider, openai_compat::CompatError};

/// Returns `Some(CompatError)` when the configured route would join
/// `/v1/responses` to the Chat family base, `None` otherwise.
pub(in crate::worker::runtime) fn check_glm_responses_base(
    base_url: &str,
    provider: EndpointProvider,
    native_api: NativeApi,
) -> Option<CompatError> {
    if provider != EndpointProvider::Glm {
        return None;
    }
    if native_api != NativeApi::Responses {
        return None;
    }
    if !is_glm_chat_family_base(base_url) {
        return None;
    }
    Some(CompatError::new(
        StatusCode::BAD_REQUEST,
        "glm_responses_incompatible_base",
        "GLM Responses requires a separate upstream row whose base is \
         https://open.bigmodel.cn/api/v1; the configured base is the \
         Chat family base (.../api/coding/paas/...) and joining \
         /responses to it always returns 404. Add a dedicated upstream \
         row for the Responses protocol and point this route at it \
         instead of reusing the Chat endpoint.",
    ))
}

/// True when the configured base looks like the Zhipu Chat family root
/// (`/api/coding/paas[/...]`). The check is path-prefix based and is
/// deliberately conservative so the fail-fast only fires for bases that
/// would build the broken `<base>/responses` URL.
fn is_glm_chat_family_base(base_url: &str) -> bool {
    let path = base_url_path(base_url);
    path.split('/').any(|segment| segment == "coding")
        && path.split('/').any(|segment| segment == "paas")
}

fn base_url_path(base_url: &str) -> String {
    if let Ok(parsed) = reqwest::Url::parse(base_url.trim()) {
        return parsed.path().to_string();
    }
    let without_query = base_url.split(['?', '#']).next().unwrap_or(base_url).trim();
    let without_scheme = if let Some(idx) = without_query.find("://") {
        &without_query[idx + 3..]
    } else {
        without_query
    };
    let path_start = without_scheme.find('/').unwrap_or(without_scheme.len());
    without_scheme[path_start..]
        .split('?')
        .next()
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{base_url_path, check_glm_responses_base, is_glm_chat_family_base};
    use crate::{config::NativeApi, db::EndpointProvider};
    use http::StatusCode;

    #[test]
    fn fail_fast_for_glm_responses_on_chat_family_base() {
        // The exact live-broken configuration: Chat family base +
        // Responses protocol. The pinned message must point operators
        // at the Responses-only base and not pretend the URL was built.
        for base in [
            "https://open.bigmodel.cn/api/coding/paas/v4",
            "https://open.bigmodel.cn/api/coding/paas/v4/",
            "https://api.z.ai/api/coding/paas/v4",
        ] {
            let err = check_glm_responses_base(base, EndpointProvider::Glm, NativeApi::Responses)
                .expect("fail-fast must trigger on Chat family base");
            assert_eq!(err.code, "glm_responses_incompatible_base");
            assert_eq!(err.status, StatusCode::BAD_REQUEST);
            assert!(
                err.message.contains("https://open.bigmodel.cn/api/v1"),
                "fail-fast message must name the Responses-only base, got: {}",
                err.message,
            );
            assert!(
                err.message.contains("coding/paas"),
                "fail-fast message must echo the offending base family, got: {}",
                err.message,
            );
        }
    }

    #[test]
    fn passes_through_correct_glm_responses_v1_base() {
        // The documented GLM Responses base must not trip the check;
        // the existing `glm_responses_base_strips_v1_prefix` test
        // covers the URL composer, and this guards the validator.
        let err = check_glm_responses_base(
            "https://open.bigmodel.cn/api/v1",
            EndpointProvider::Glm,
            NativeApi::Responses,
        );
        assert!(err.is_none(), "correct Responses base must pass through");
    }

    #[test]
    fn anthropic_join_on_chat_base_is_not_a_responses_route() {
        // GLM Anthropic base does not contain `/coding/paas`, so even
        // if the operator wires it to the Chat base (an unusual but
        // legitimate choice for `/v1/messages`), the validator must
        // not fire — that is a Chat-family path, not a Responses one.
        let err = check_glm_responses_base(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            EndpointProvider::Glm,
            NativeApi::AnthropicMessages,
        );
        assert!(err.is_none(), "Anthropic on Chat base is not Responses");
    }

    #[test]
    fn chat_join_on_chat_base_is_not_a_responses_route() {
        let err = check_glm_responses_base(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            EndpointProvider::Glm,
            NativeApi::Chat,
        );
        assert!(err.is_none(), "Chat on Chat base must not be flagged");
    }

    #[test]
    fn realtime_on_chat_base_is_not_a_responses_route() {
        // Realtime shares the Chat family base by design; the
        // fail-fast must not bleed into the Realtime path.
        let err = check_glm_responses_base(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            EndpointProvider::Glm,
            NativeApi::Realtime,
        );
        assert!(err.is_none(), "Realtime on Chat base must not be flagged");
    }

    #[test]
    fn non_glm_responses_on_chat_family_base_is_untouched() {
        // Generic / MiniMax providers may legitimately point at any
        // base; the GLM-only check must not bleed across providers.
        for provider in [EndpointProvider::Generic, EndpointProvider::Minimax] {
            let err = check_glm_responses_base(
                "https://open.bigmodel.cn/api/coding/paas/v4",
                provider,
                NativeApi::Responses,
            );
            assert!(err.is_none(), "{provider:?} Responses must not be flagged");
        }
    }

    #[test]
    fn auto_resolved_responses_is_covered_by_native_api_check() {
        // Auto is resolved to a concrete `NativeApi` (Chat / Responses /
        // AnthropicMessages) before reaching the validator via
        // `process_request::resolve_auto_protocol`, so the validator
        // only ever sees a concrete arm. Pin that an unresolved Auto
        // is treated like a non-Responses arm and passes through.
        let err = check_glm_responses_base(
            "https://open.bigmodel.cn/api/coding/paas/v4",
            EndpointProvider::Glm,
            NativeApi::Auto,
        );
        assert!(
            err.is_none(),
            "Auto is resolved upstream and must not trip the Responses check",
        );
    }

    #[test]
    fn chat_family_base_detector_recognises_paas_segment() {
        assert!(is_glm_chat_family_base(
            "https://open.bigmodel.cn/api/coding/paas/v4"
        ));
        assert!(is_glm_chat_family_base(
            "https://api.z.ai/api/coding/paas/v4/"
        ));
        assert!(!is_glm_chat_family_base("https://open.bigmodel.cn/api/v1"));
        assert!(!is_glm_chat_family_base(
            "https://open.bigmodel.cn/api/anthropic"
        ));
    }

    #[test]
    fn base_url_path_handles_querystring_and_unparseable_inputs() {
        assert_eq!(
            base_url_path("https://open.bigmodel.cn/api/coding/paas/v4?model=x"),
            "/api/coding/paas/v4"
        );
        // Unparseable scheme-less input: fall back to a host/path split
        // so the check is robust against operator typos.
        assert_eq!(
            base_url_path("open.bigmodel.cn/api/coding/paas/v4"),
            "/api/coding/paas/v4"
        );
    }
}
