//! Issue #599 R2f.1: ChatGPT OAuth access-token JWT claim extraction.
//!
//! The subscription access token is a JWT whose claims bind a request to a
//! ChatGPT account and, optionally, a compute residency. Only the payload is
//! decoded here — the token comes from our own OAuth exchange/refresh, so its
//! signature is not re-verified — and claim values are never stored or logged.

use base64::{Engine as _, engine::general_purpose::URL_SAFE};
use serde_json::Value;

/// Namespaced claim container used by the ChatGPT OAuth access token.
const AUTH_CLAIM_NAMESPACE: &str = "https://api.openai.com/auth";
/// Account id claim, read top level first and under the auth namespace second.
const ACCOUNT_ID_CLAIM: &str = "chatgpt_account_id";
/// Compute-residency claim, read under the auth namespace first (a namespaced
/// value wins over a top-level one) and top level second.
const COMPUTE_RESIDENCY_CLAIM: &str = "chatgpt_compute_residency";
/// Sentinel meaning "no residency constraint"; never forwarded upstream.
const NO_RESIDENCY_CONSTRAINT: &str = "no_constraint";

/// ChatGPT account id carried by the access token. `None` when the token is
/// opaque or malformed.
pub(super) fn account_id_from_access_token(access_token: &str) -> Option<String> {
    let payload = decode_payload(access_token)?;
    let account_id = payload
        .get(ACCOUNT_ID_CLAIM)
        .or_else(|| namespaced_claim(&payload, ACCOUNT_ID_CLAIM));
    trimmed_string(account_id)
}

/// Compute residency carried by the access token. `None` when the token is
/// opaque or malformed, or when the claim is missing, blank, or
/// `no_constraint`; the value is never persisted on the credential.
pub(super) fn compute_residency_from_access_token(access_token: &str) -> Option<String> {
    let payload = decode_payload(access_token)?;
    let residency = namespaced_claim(&payload, COMPUTE_RESIDENCY_CLAIM)
        .or_else(|| payload.get(COMPUTE_RESIDENCY_CLAIM));
    trimmed_string(residency).filter(|value| value != NO_RESIDENCY_CONSTRAINT)
}

fn namespaced_claim<'a>(payload: &'a Value, claim: &str) -> Option<&'a Value> {
    payload
        .get(AUTH_CLAIM_NAMESPACE)
        .and_then(|auth| auth.get(claim))
}

fn trimmed_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// JWT payload of `<header>.<payload>.<signature>`, tolerant of unpadded
/// base64url segments.
fn decode_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let mut padded = payload.to_string();
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    let bytes = URL_SAFE.decode(padded.as_bytes()).ok()?;
    serde_json::from_slice::<Value>(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt_with_claims(claims: serde_json::Value) -> String {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    #[test]
    fn account_id_reads_direct_then_namespaced_claims() {
        let direct = jwt_with_claims(serde_json::json!({
            "chatgpt_account_id": "direct-id",
            "https://api.openai.com/auth": { "chatgpt_account_id": "nested-id" },
        }));
        assert_eq!(
            account_id_from_access_token(&direct).as_deref(),
            Some("direct-id")
        );
        let nested_only = jwt_with_claims(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": " nested-id " },
        }));
        assert_eq!(
            account_id_from_access_token(&nested_only).as_deref(),
            Some("nested-id")
        );
    }

    #[test]
    fn account_id_is_skipped_for_unreadable_claims() {
        assert_eq!(account_id_from_access_token("opaque-token"), None);
        assert_eq!(account_id_from_access_token("header.!!!.signature"), None);
        assert_eq!(
            account_id_from_access_token(&jwt_with_claims(serde_json::json!({
                "chatgpt_account_id": "   ",
            }))),
            None
        );
    }

    #[test]
    fn residency_reads_namespaced_then_top_level_claims() {
        let nested = jwt_with_claims(serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_compute_residency": "eu" },
        }));
        assert_eq!(
            compute_residency_from_access_token(&nested).as_deref(),
            Some("eu")
        );
        let top_level = jwt_with_claims(serde_json::json!({
            "chatgpt_compute_residency": "future-region_1",
        }));
        assert_eq!(
            compute_residency_from_access_token(&top_level).as_deref(),
            Some("future-region_1")
        );
    }

    #[test]
    fn residency_ignores_no_constraint_blank_and_unreadable_claims() {
        let unconstrained_namespaced = jwt_with_claims(serde_json::json!({
            "chatgpt_compute_residency": "eu",
            "https://api.openai.com/auth": { "chatgpt_compute_residency": "no_constraint" },
        }));
        assert_eq!(
            compute_residency_from_access_token(&unconstrained_namespaced),
            None
        );
        for claims in [
            serde_json::json!({ "chatgpt_compute_residency": "no_constraint" }),
            serde_json::json!({ "chatgpt_compute_residency": "  " }),
            serde_json::json!({
                "https://api.openai.com/auth": { "chatgpt_compute_residency": 7 },
            }),
            serde_json::json!({ "chatgpt_data_residency": "gb" }),
        ] {
            assert_eq!(
                compute_residency_from_access_token(&jwt_with_claims(claims)),
                None
            );
        }
        assert_eq!(compute_residency_from_access_token("opaque-token"), None);
    }
}
