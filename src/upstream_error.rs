use serde_json::Value;

/// Why one upstream non-success response is being classified. The cause only
/// refines presentation and analytics: every 429 still backs the route off,
/// and an unknown 429 is a rate limit rather than a claim of exhaustion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UpstreamFailureCause {
    /// The upstream explicitly confirms the quota or balance is spent.
    Exhausted,
    /// Throttling, concurrency or an in-flight reservation — transient.
    RateLimited,
    /// The upstream explicitly says it cannot serve the request right now.
    UpstreamUnavailable,
}

/// Status-aware classification of one upstream failure body.
///
/// A success body never classifies as a failure, an unrelated non-429 failure
/// keeps its existing handling (`None`), and a 429 whose body could not be read
/// or understood degrades to a generic rate limit instead of claiming
/// exhaustion.
pub(crate) fn classify_upstream_failure(status: u16, body: &str) -> Option<UpstreamFailureCause> {
    if (200..300).contains(&status) {
        return None;
    }
    match classify_upstream_body(body) {
        cause @ Some(_) => cause,
        None if status == 429 => Some(UpstreamFailureCause::RateLimited),
        None => None,
    }
}

/// Token Plan rate-limit code. Exhaustion uses the neighbouring 2056/2067.
const RATE_LIMIT_PROVIDER_CODE: u32 = 2062;
/// Provider codes that state the account's usage is spent.
const EXHAUSTED_PROVIDER_CODES: &[u32] = &[2056, 2067];

/// Transient phrases only. Bare `quota`, `credit`, `billing`, `token plan`,
/// `额度` and HTTP 429/402 are deliberately absent: they describe throttling
/// as often as they describe a spent account.
const RATE_LIMIT_PHRASES: &[&str] = &[
    "rate limit",
    "rate limits",
    "too many requests",
    "request rate",
    "requests per minute",
    "token rate",
    "tokens per minute",
    "tpm limit",
    "rpm limit",
    "concurrency limit",
    "concurrent requests",
    "too many concurrent",
    "throttl",
    // An in-flight reservation is a transient hold, not a spent balance.
    "in flight",
    "inflight",
    "credit reservation",
    "reserved credits",
    "pending requests",
];

/// Explicit unavailability only. A generic 5xx or an authentication failure is
/// not an outage and keeps its own handling.
const UNAVAILABLE_PHRASES: &[&str] = &[
    "service unavailable",
    "temporarily unavailable",
    "temporarily unable",
    "temporarily not available",
    "upstream unavailable",
    "endpoint unavailable",
    "provider unavailable",
    "model unavailable",
    "no available provider",
    "no available capacity",
    "out of service",
    "under maintenance",
    "overloaded",
];

/// Exhaustion needs an explicit statement, never a bare subject noun: an
/// upstream that says `quota`, `credit`, `billing` or `额度` on its own has not
/// said anything about how much is left.
const EXHAUSTED_PHRASES: &[&str] = &[
    "insufficient quota",
    "insufficient balance",
    "insufficient credits",
    "insufficient funds",
    "usage limit reached",
    "usage limit exceeded",
    "usage limit exhausted",
    "quota exhausted",
    "quota exceeded",
    "credit limit exhausted",
    "credits exhausted",
    "credit exhausted",
    // OpenCode Go reports the account's allowance rather than a remaining
    // amount; the typed envelope is handled by the phrase below plus its code.
    "gousagelimiterror",
    "monthly usage limit",
    "weekly usage limit",
    "daily usage limit",
    "free usage limit",
    "额度不足",
    "余额不足",
    "额度已用尽",
    "额度耗尽",
    "用量上限已用尽",
    "用量上限已耗尽",
    "用量已达上限",
];

fn classify_upstream_body(body: &str) -> Option<UpstreamFailureCause> {
    // Provider codes and phrases also travel in underscore/hyphen form
    // (`free_usage_limit`, `rate-limit`), so every table is matched against a
    // separator-folded copy of the response.
    let folded = fold(body);
    // A payload that parses is read through its recognized structured code
    // fields only. Scanning the serialized text for a key-shaped marker would
    // let `{"zipcode":2056}` reach the fallback, so the fallback is reserved
    // for a body that never parsed at all.
    let parsed = serde_json::from_str::<Value>(body).ok();
    let structured = parsed.as_ref().map(structured_codes);
    let mentions = |wanted: u32| match &structured {
        Some(codes) => mentions_structured_code(codes, wanted),
        None => mentions_free_text_code(&folded, wanted),
    };
    // Issue #1179 P2 repair: the message fallback needs the verified MiniMax
    // exhaustion phrase together with a closed 2056/2067 code, and an explicit
    // structured code that names another failure wins over the body text.
    let explicit_conflict = structured.as_ref().is_some_and(|codes| {
        codes
            .iter()
            .any(|code| !is_generic_or_exhaustion_code(code))
    });
    let message_exhaustion = || {
        parsed
            .as_ref()
            .and_then(message_exhaustion_signature)
            .is_some()
    };
    // Transient and unavailable signals win over every exhaustion name: an
    // in-flight reservation or an explicit outage that happens to mention
    // quota/credits is not a depleted account.
    if mentions(RATE_LIMIT_PROVIDER_CODE)
        || RATE_LIMIT_PHRASES
            .iter()
            .any(|phrase| folded.contains(phrase))
    {
        return Some(UpstreamFailureCause::RateLimited);
    }
    if UNAVAILABLE_PHRASES
        .iter()
        .any(|phrase| folded.contains(phrase))
    {
        return Some(UpstreamFailureCause::UpstreamUnavailable);
    }
    if EXHAUSTED_PROVIDER_CODES.iter().copied().any(mentions)
        || (!explicit_conflict
            && (message_exhaustion()
                || EXHAUSTED_PHRASES
                    .iter()
                    .any(|phrase| folded.contains(phrase))))
    {
        return Some(UpstreamFailureCause::Exhausted);
    }
    None
}

/// The only field names that carry a provider code, matched whole after
/// separator folding. `zipcode` or `discount_code` are different keys and are
/// never one of these.
const CODE_KEYS: &[&str] = &["code", "error code", "provider code", "provider error code"];

/// Characters that may sit between a recognized key and its value without
/// ending it: JSON quoting, the usual key/value separators and whitespace.
const KEY_VALUE_SEPARATORS: &[char] = &[' ', '\t', '"', '\'', ':', '=', '.'];

fn mentions_structured_code(codes: &[String], wanted: u32) -> bool {
    codes
        .iter()
        .any(|code| code.parse::<u32>().is_ok_and(|code| code == wanted))
}

/// Free-text and truncated-envelope fallback. The recognized key must match
/// whole and the value must be exactly the code, closed by a real terminator,
/// so `20560`, `2056abc` and `2067foo` never reach 2056/2067.
fn mentions_free_text_code(folded: &str, wanted: u32) -> bool {
    let wanted = wanted.to_string();
    CODE_KEYS.iter().any(|key| {
        folded.match_indices(key).any(|(start, key)| {
            at_key_start(folded, start)
                && code_value_after_key(&folded[start + key.len()..])
                    .is_some_and(|value| value == wanted)
        })
    }) || parenthesized_code(folded, &wanted)
}

/// A code stands alone as its own value only when what follows the digits is
/// not part of it: no trailing digit and no trailing letter.
fn code_value_after_key(rest: &str) -> Option<String> {
    let value = rest.trim_start_matches(KEY_VALUE_SEPARATORS);
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || !ends_value(&value[digits.len()..]) {
        return None;
    }
    Some(digits)
}

/// The known plain-text parenthesized form, closed on both sides.
///
/// The provider writes the code with ASCII or full-width parentheses.
fn parenthesized_code(folded: &str, wanted: &str) -> bool {
    folded.match_indices(['(', '（']).any(|(start, open)| {
        let value = &folded[start + open.len()..];
        value.starts_with(wanted)
            && (value[wanted.len()..].starts_with(')') || value[wanted.len()..].starts_with('）'))
    })
}

/// A recognized key only starts where a field name can start: at the beginning
/// of the body or right after an enclosing delimiter. `zip code` and
/// `zipcode` are therefore not the key `code`.
fn at_key_start(text: &str, start: usize) -> bool {
    matches!(
        text[..start].trim_end().chars().next_back(),
        None | Some('{') | Some('[') | Some('(') | Some(',') | Some(':') | Some('"') | Some('\'')
    )
}

/// What may follow a code's digits for them to be the whole value.
fn ends_value(rest: &str) -> bool {
    matches!(
        rest.chars().next(),
        None | Some(' ')
            | Some('\t')
            | Some('\n')
            | Some(')')
            | Some('"')
            | Some('\'')
            | Some(',')
            | Some('}')
            | Some(']')
            | Some(';')
    )
}

fn fold(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| match character {
            '_' | '-' => ' ',
            other => other,
        })
        .collect()
}

/// Every provider code in an upstream payload, at any nesting depth.
fn structured_codes(value: &Value) -> Vec<String> {
    let mut codes = Vec::new();
    collect_codes(value, &mut codes);
    codes
}

/// The verified MiniMax Token Plan exhaustion wording.
///
/// The message fallback only fires on one of these phrases together with a
/// closed 2056/2067 code, so an unrelated number, a quoted example or a
/// different failure never claims exhaustion. The phrases are the Chinese
/// usage-limit wording already documented for this provider and its narrow
/// English equivalents.
const MESSAGE_EXHAUSTION_PHRASES: &[&str] = &[
    // The wording the provider currently sends for a spent Token Plan
    // allowance, case-folded: `已达到 Token Plan 用量上限：...`.
    "已达到 token plan 用量上限",
    "用量已达上限",
    "用量上限已用尽",
    "用量上限已耗尽",
    "额度已用尽",
    "额度耗尽",
    "usage limit reached",
    "usage limit exceeded",
    "usage limit exhausted",
];

/// The generic code values a provider message may refine: `server_error`
/// states no cause, so the verified message signature still decides. Codes are
/// compared after separator folding, so the entry is the folded form.
const GENERIC_PROVIDER_CODES: &[&str] = &["server error"];

/// Whether an explicit code names the known generic failure or an exhausted
/// quota.
///
/// The exhaustion names are the existing phrase table itself, compared exactly
/// after separator folding, so a provider that writes its code as
/// `quota_exceeded` or `insufficient_credits` reaches the same statement as the
/// sentence form — without a second hand-maintained name list. A code that only
/// extends a recognized phrase (`quota_exceeded_validation_error`) is a
/// different, conflicting cause.
fn is_generic_or_exhaustion_code(code: &str) -> bool {
    let folded = fold(code);
    let code = folded.trim();
    GENERIC_PROVIDER_CODES.contains(&code)
        || EXHAUSTED_PHRASES.contains(&code)
        || EXHAUSTED_PROVIDER_CODES
            .iter()
            .any(|wanted| code == wanted.to_string())
}

/// The exhaustion code of a provider message that carries the verified phrase
/// and a closed 2056/2067 code.
fn message_exhaustion_signature(value: &Value) -> Option<u32> {
    structured_messages(value).into_iter().find_map(|message| {
        let folded = fold(&message);
        if !MESSAGE_EXHAUSTION_PHRASES
            .iter()
            .any(|phrase| folded.contains(phrase))
        {
            return None;
        }
        EXHAUSTED_PROVIDER_CODES
            .iter()
            .copied()
            .find(|code| parenthesized_code(&folded, &code.to_string()))
    })
}

/// The field names that carry a provider message.
const MESSAGE_KEYS: &[&str] = &["message", "error message"];

/// Every provider message in an upstream payload, at any nesting depth.
fn structured_messages(value: &Value) -> Vec<String> {
    let mut messages = Vec::new();
    collect_messages(value, &mut messages);
    messages
}

fn collect_messages(value: &Value, messages: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                if MESSAGE_KEYS.contains(&fold(key).trim())
                    && let Some(text) = field.as_str()
                {
                    messages.push(text.to_string());
                }
                collect_messages(field, messages);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_messages(value, messages);
            }
        }
        _ => {}
    }
}

fn collect_codes(value: &Value, codes: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                if is_code_key(key)
                    && let Some(scalar) = scalar_text(field)
                {
                    codes.push(scalar);
                }
                collect_codes(field, codes);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_codes(value, codes);
            }
        }
        _ => {}
    }
}

fn is_code_key(key: &str) -> bool {
    CODE_KEYS.contains(&fold(key).trim())
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(number.to_string()),
        Value::String(value) => Some(value.trim().to_string()),
        _ => None,
    }
}

/// A recoverable upstream `invalid_request` caused by a stale Responses
/// continuation: the client kept `previous_response_id` (or a thread-scoped
/// `rs_*` item) that the upstream has already expired or pruned.
pub struct MappedInvalid {
    pub code: &'static str,
    pub hint: String,
    pub retryable: bool,
}

pub fn map_upstream_invalid_request(body: &str) -> Option<MappedInvalid> {
    let has_missing_output = body.contains("No tool output found for function call");
    let has_expired_reasoning =
        body.contains("was not found or has expired") && body.contains("reasoning item");
    if !has_missing_output && !has_expired_reasoning {
        return None;
    }
    Some(MappedInvalid {
        code: "retryable_invalid_continuation",
        hint: "drop previous_response_id, truncate before the orphan call_*, then retry once"
            .to_string(),
        retryable: true,
    })
}

#[cfg(test)]
mod tests {
    use super::{UpstreamFailureCause, classify_upstream_failure};

    const EXHAUSTED: Option<UpstreamFailureCause> = Some(UpstreamFailureCause::Exhausted);
    const RATE_LIMITED: Option<UpstreamFailureCause> = Some(UpstreamFailureCause::RateLimited);
    const UNAVAILABLE: Option<UpstreamFailureCause> =
        Some(UpstreamFailureCause::UpstreamUnavailable);

    #[test]
    fn recognizes_explicit_exhaustion_across_provider_formats() {
        for body in [
            "insufficient_quota",
            "Token Plan usage limit exhausted",
            "用量上限已用尽",
            "当前额度不足",
            r#"{"error":{"code":2056,"message":"provider limit"}}"#,
            "provider code: 2056",
            // DeepSeek 402: the raw upstream body has no quota keyword.
            r#"{"error":{"message":"Insufficient Balance","type":"unknown_error"}}"#,
            // OpenRouter 402 renders its own credit wording.
            "OpenRouter credit limit exhausted (quota)",
            "账户余额不足，请充值",
            r#"{"error":{"code":2067,"message":"account level"}}"#,
        ] {
            assert_eq!(classify_upstream_failure(429, body), EXHAUSTED, "{body}");
        }
    }

    #[test]
    fn recognizes_the_opencode_go_usage_limit_envelope() {
        // Real `request_records` body (issue #310): the typed code and the
        // monthly message must both classify as quota exhaustion, and the
        // typed code alone must be enough if the message is reworded.
        let body = r#"{"type":"error","error":{"type":"GoUsageLimitError","message":"Monthly usage limit reached. Resets in 4 days."},"metadata":{"limitName":"monthly"}}"#;
        assert_eq!(classify_upstream_failure(429, body), EXHAUSTED);
        assert_eq!(
            classify_upstream_failure(429, "GoUsageLimitError"),
            EXHAUSTED
        );
        assert_eq!(
            classify_upstream_failure(429, "Free usage limit reached"),
            EXHAUSTED
        );
    }

    #[test]
    fn never_treats_a_bare_quota_credit_or_billing_word_as_exhaustion() {
        for body in [
            "quota",
            "credit",
            "billing",
            "token plan",
            "额度",
            r#"{"error":{"message":"quota information is unavailable"}}"#,
        ] {
            assert_ne!(classify_upstream_failure(429, body), EXHAUSTED, "{body}");
        }
        // Neither HTTP 429 nor HTTP 402 carries exhaustion evidence by itself,
        // and an unrelated status keeps its existing handling.
        assert_eq!(classify_upstream_failure(429, ""), RATE_LIMITED);
        assert_eq!(classify_upstream_failure(402, "Payment Required"), None);
        assert_eq!(classify_upstream_failure(400, "Bad Request"), None);
        assert_eq!(classify_upstream_failure(401, "Unauthorized"), None);
    }

    #[test]
    fn transient_signals_take_precedence_over_exhaustion_names() {
        for body in [
            r#"{"error":{"code":2062,"message":"Token Plan request rate limit"}}"#,
            "in-flight credits reserved for this request",
            "Inflight reservation; account quota untouched",
            "token rate limit exceeded",
            "rate limit reached for requests per minute",
            "concurrency limit reached",
            "pending requests in queue",
        ] {
            assert_eq!(classify_upstream_failure(429, body), RATE_LIMITED, "{body}");
        }
        // A balance type on its own proves nothing; only an explicit statement
        // does, and then it is exhaustion.
        assert_eq!(
            classify_upstream_failure(429, r#"{"error":{"type":"CreditsError"}}"#),
            RATE_LIMITED
        );
        assert_eq!(
            classify_upstream_failure(
                429,
                r#"{"error":{"type":"CreditsError","message":"Insufficient Balance"}}"#
            ),
            EXHAUSTED
        );
        assert_eq!(
            classify_upstream_failure(
                400,
                r#"{"error":{"type":"billing_error","message":"insufficient_quota"}}"#
            ),
            EXHAUSTED
        );
    }

    #[test]
    fn explicit_unavailability_is_neither_quota_nor_rate_limit() {
        for body in [
            "Service Unavailable",
            "The endpoint is temporarily unavailable",
            "upstream provider unavailable",
            "model overloaded, try again later",
        ] {
            assert_eq!(classify_upstream_failure(429, body), UNAVAILABLE, "{body}");
        }
        // A generic server error is not an outage, and a 5xx that merely
        // describes one keeps the same unavailable cause as a 429.
        assert_eq!(
            classify_upstream_failure(503, r#"{"error":{"message":"boom"}}"#),
            None
        );
        assert_eq!(
            classify_upstream_failure(503, "Service Unavailable"),
            UNAVAILABLE
        );
        assert_eq!(
            classify_upstream_failure(500, r#"{"error":{"message":"temporary provider failure"}}"#),
            None
        );
        // An authentication failure is not an outage, but an explicitly
        // described unavailability stays one whatever status carried it.
        assert_eq!(
            classify_upstream_failure(401, r#"{"error":{"message":"invalid api key"}}"#),
            None
        );
        assert_eq!(
            classify_upstream_failure(
                401,
                r#"{"error":{"message":"temporarily unavailable key"}}"#
            ),
            UNAVAILABLE
        );
    }

    #[test]
    fn a_provider_code_matches_only_on_exact_numeric_boundaries() {
        // A longer numeric code is a different code, in every spelling and in
        // both a parsed and an unparseable body.
        for body in [
            r#"{"error":{"code":20560,"message":"unrelated failure"}}"#,
            r#"{"error":{"code":"20560","message":"unrelated failure"}}"#,
            r#"{"error":{"code":20670,"message":"unrelated failure"}}"#,
            r#"{"error":{"code":"20670"}}"#,
            r#"{"error":{"code":20620,"message":"unrelated failure"}}"#,
            r#"{"error":{"code":"20620"}}"#,
            "provider code: 20560",
            "provider code=20560",
            "provider code 20560",
            "quota (20560)",
            r#"{"error":{"code":20670"#,
            r#"{"code":20560"#,
            r#"{"error":{"code": 20560}}"#,
        ] {
            assert_ne!(
                classify_upstream_failure(400, body),
                EXHAUSTED,
                "{body} must not read as exhaustion"
            );
            assert_ne!(
                classify_upstream_failure(400, body),
                RATE_LIMITED,
                "{body} must not read as a rate limit"
            );
        }
        // The exact codes still classify, parsed and truncated alike.
        for body in [
            r#"{"error":{"code":2056,"message":"provider limit"}}"#,
            r#"{"error":{"code":"2056"}}"#,
            r#"{"error":{"code":2067}}"#,
            r#"{"error":{"code":"2062","message":"request rate limit"}}"#,
            "provider code: 2056",
            "provider code=2067",
            "quota (2056)",
            r#"{"error":{"code":2056"#,
        ] {
            let cause = classify_upstream_failure(400, body);
            assert!(
                matches!(cause, EXHAUSTED | RATE_LIMITED),
                "{body} must still classify, got {cause:?}"
            );
        }
        assert_eq!(
            classify_upstream_failure(400, r#"{"error":{"code":"2062"}}"#),
            RATE_LIMITED
        );
        assert_eq!(
            classify_upstream_failure(400, r#"{"code":2062"#),
            RATE_LIMITED
        );
    }

    #[test]
    fn a_provider_code_is_read_only_through_a_whole_recognized_key() {
        // An unrelated key whose name merely ends in "code" is a different key:
        // a parsed payload is read through its structured fields only, so none
        // of these may reach the free-text fallback at all.
        for body in [
            r#"{"zipcode":2056,"message":"unrelated failure"}"#,
            r#"{"zipcode":"2056"}"#,
            r#"{"error":{"discount_code":2067}}"#,
            r#"{"error":{"codes":2056}}"#,
            r#"{"error":{"zipcode":2062}}"#,
            r#"{"error":{"provider_status_code":2056}}"#,
        ] {
            assert_eq!(
                classify_upstream_failure(400, body),
                None,
                "{body} must not classify at all"
            );
        }
        // The same rule in free text and in a truncated envelope: the key must
        // be a recognized one, whole, with the code standing alone as its value.
        for body in [
            "zip code: 2056",
            "discount_code: 2067",
            "zipcode: 2062",
            "codes: 2056",
            r#"{"zipcode":2056"#,
            r#"{"error":{"discount_code":2067"#,
            // Trailing digits and trailing letters are not a terminated value.
            "code: 20560",
            "code: 2056abc",
            "code: 2067foo",
            "code: 2062x",
            r#"{"error":{"code":2056abc"#,
            r#"{"error":{"code": 2067foo"#,
        ] {
            assert_ne!(
                classify_upstream_failure(400, body),
                EXHAUSTED,
                "{body} must not read as exhaustion"
            );
            assert_ne!(
                classify_upstream_failure(400, body),
                RATE_LIMITED,
                "{body} must not read as a rate limit"
            );
        }
        // Recognized keys in their known plain-text and structured spellings
        // still classify, with spacing and quoting variations.
        for body in [
            "code: 2056",
            "code=2056",
            "code 2056",
            "code  =  2056",
            r#""code":2056"#,
            "error code: 2067",
            "provider code: 2056",
            "provider error code: 2056",
            "quota (2056)",
            r#"{"error":{"code":2056,"message":"provider limit"}}"#,
            r#"{"error":{"error_code":"2067"}}"#,
            r#"{"error":{"provider_error_code":2056}}"#,
            r#"{"error":{"code":"2062","message":"request rate limit"}}"#,
            r#"{"error":{"code":2056"#,
            "provider code: 2062",
        ] {
            let cause = classify_upstream_failure(400, body);
            assert!(
                matches!(cause, EXHAUSTED | RATE_LIMITED),
                "{body} must still classify, got {cause:?}"
            );
        }
    }

    #[test]
    fn an_explicit_exhaustion_classifies_the_same_at_every_original_status() {
        // The cause does not depend on the status an upstream attached: the
        // same explicit statement is exhausted at 400, 401, 402, 403, 429 and
        // 500, and only the 429 additionally backs the route off on its own.
        let bodies = [
            "insufficient_quota",
            "Monthly usage limit reached",
            "Insufficient Balance",
            r#"{"error":{"code":2056,"message":"usage limit reached"}}"#,
            "账户额度不足",
        ];
        for status in [400u16, 401, 402, 403, 429, 500] {
            for body in bodies {
                assert_eq!(
                    classify_upstream_failure(status, body),
                    EXHAUSTED,
                    "status {status}, body {body}"
                );
            }
        }
    }

    #[test]
    fn a_provider_message_with_the_verified_phrase_and_a_closed_quota_code_is_exhausted() {
        // Issue #1179 P2 repair: the MiniMax AI route answers a spent plan
        // quota with a generic error code and the provider code inside its
        // message. The fallback needs the verified exhaustion phrase together
        // with a closed 2056/2067 code.
        for body in [
            r#"{"error":{"code":"server_error","message":"用量已达上限 (2056)"}}"#,
            r#"{"error":{"code":"server_error","message":"用量已达上限（2067）"}}"#,
            r#"{"error":{"message":"usage limit reached (2056)"}}"#,
            r#"{"error":{"code":"server_error","message":"Token Plan usage limit exhausted (2056)"}}"#,
        ] {
            assert_eq!(classify_upstream_failure(500, body), EXHAUSTED, "{body}");
        }
    }

    #[test]
    fn a_message_code_without_the_verified_phrase_is_not_exhaustion() {
        // The numeric fallback alone never claims exhaustion: the closed code
        // must sit beside the verified phrase.
        for body in [
            r#"{"error":{"code":"server_error","message":"record (2056) invalid"}}"#,
            r#"{"error":{"code":"server_error","message":"request failed (2056)"}}"#,
            r#"{"error":{"code":"server_error","message":"see the example \"(2056)\" in the guide"}}"#,
            r#"{"error":{"code":"server_error","message":"failed (20560)"}}"#,
            r#"{"error":{"code":"server_error","message":"failed (2056 items)"}}"#,
            r#"{"error":{"code":"server_error","message":"failed 2056"}}"#,
            r#"{"error":{"code":"invalid_request_error","message":"用量已达上限 (2056)"}}"#,
        ] {
            assert_ne!(classify_upstream_failure(500, body), EXHAUSTED, "{body}");
        }
    }

    #[test]
    fn every_recognized_exhaustion_phrase_is_accepted_as_its_structured_code() {
        // The structured code names are the phrase table itself: a provider
        // that writes any recognized statement as `error.code` must classify
        // the same way the sentence form does, at a rejecting status.
        for phrase in super::EXHAUSTED_PHRASES {
            for status in [400_u16, 429] {
                let body = format!(r#"{{"error":{{"code":"{phrase}","message":"{phrase}"}}}}"#);
                assert_eq!(
                    classify_upstream_failure(status, &body),
                    EXHAUSTED,
                    "{status} {body}"
                );
            }
        }
    }

    #[test]
    fn a_structured_code_must_match_a_recognized_phrase_exactly() {
        // Extending a known phrase with another cause is a different code and
        // still blocks the message fallback.
        for code in [
            "quota_exceeded_validation_error",
            "insufficient_credits_deprecated",
            "usage_limit_reached_retry",
            "invalid_api_key",
        ] {
            let body =
                format!(r#"{{"error":{{"code":"{code}","message":"用量已达上限 (2056)"}}}}"#);
            assert_ne!(classify_upstream_failure(400, &body), EXHAUSTED, "{body}");
        }
    }

    #[test]
    fn the_observed_token_plan_wording_is_an_exhaustion_signature() {
        // The exact body observed in production: a generic code, the Chinese
        // Token Plan wording and the closed 2056 code.
        let body = r#"{"error":{"code":"server_error","message":"已达到 Token Plan 用量上限：请升级 Token Plan 套餐或购买积分补充用量。 (2056)"}}"#;
        for status in [400_u16, 429, 500] {
            assert_eq!(
                classify_upstream_failure(status, body),
                EXHAUSTED,
                "{status}"
            );
        }
        // A bare mention of Token Plan, or an incidental 2056 without the
        // verified phrase, never claims exhaustion.
        for body in [
            r#"{"error":{"code":"server_error","message":"Token Plan 套餐信息已更新"}}"#,
            r#"{"error":{"code":"server_error","message":"Token Plan 请求失败 (2056)"}}"#,
        ] {
            assert_ne!(classify_upstream_failure(400, body), EXHAUSTED, "{body}");
        }
    }

    #[test]
    fn a_known_exhaustion_code_is_not_a_conflict() {
        // The OpenAI-compatible `insufficient_quota` code names the spent
        // balance itself, so an explicit rejection carrying it is exhaustion
        // without a 2056/2067 number; a 400 status does not change that.
        for (status, body) in [
            (
                400,
                r#"{"error":{"code":"insufficient_quota","message":"You exceeded your current quota"}}"#,
            ),
            (429, r#"{"error":{"code":"insufficient_quota"}}"#),
            (402, r#"{"error":{"code":"insufficient_balance"}}"#),
        ] {
            assert_eq!(
                classify_upstream_failure(status, body),
                EXHAUSTED,
                "{status} {body}"
            );
        }
        // The pre-existing recognized equivalents keep their classification
        // when the provider writes them as the structured code: the frozen
        // baseline read these bodies as exhaustion through the phrase table.
        for code in [
            "quota_exceeded",
            "insufficient_credits",
            "insufficient_funds",
            "usage_limit_reached",
            "credit_limit_exhausted",
        ] {
            let body = format!(
                r#"{{"error":{{"code":"{code}","message":"{}"}}}}"#,
                code.replace('_', " ")
            );
            assert_eq!(classify_upstream_failure(400, &body), EXHAUSTED, "{body}");
        }
        // Every other explicit code still blocks the message fallback.
        for body in [
            r#"{"error":{"code":"invalid_api_key","message":"用量已达上限 (2056)"}}"#,
            r#"{"error":{"code":"insufficient_quota_deprecated","message":"用量已达上限 (2056)"}}"#,
        ] {
            assert_ne!(classify_upstream_failure(400, body), EXHAUSTED, "{body}");
        }
    }

    #[test]
    fn a_conflicting_explicit_code_wins_over_the_message() {
        for body in [
            r#"{"error":{"code":"invalid_request_error","message":"用量已达上限 (2056)"}}"#,
            r#"{"error":{"code":1000,"message":"用量已达上限 (2056)"}}"#,
            r#"{"error":{"code":"authentication_error","message":"quota exceeded (2056)"}}"#,
        ] {
            assert_ne!(classify_upstream_failure(500, body), EXHAUSTED, "{body}");
        }
        // The generic code and the absent code do not conflict.
        for body in [
            r#"{"error":{"code":"server_error","message":"用量已达上限 (2056)"}}"#,
            r#"{"error":{"message":"用量已达上限 (2056)"}}"#,
        ] {
            assert_eq!(classify_upstream_failure(500, body), EXHAUSTED, "{body}");
        }
    }

    #[test]
    fn the_direct_structured_code_keeps_its_precedence() {
        for body in [
            r#"{"error":{"code":2056,"message":"unrelated"}}"#,
            r#"{"error":{"provider_code":2067,"message":"unrelated"}}"#,
        ] {
            assert_eq!(classify_upstream_failure(500, body), EXHAUSTED, "{body}");
        }
        // A success body keeps its meaning whatever it carries.
        assert_eq!(
            classify_upstream_failure(
                200,
                r#"{"error":{"code":"server_error","message":"用量已达上限 (2056)"}}"#
            ),
            None
        );
    }

    #[test]
    fn covers_every_observed_production_class_and_the_degenerate_inputs() {
        // The eight classes observed in the production 429 history.
        for (body, expected) in [
            ("Too Many Requests", RATE_LIMITED),
            ("request rate limit exceeded", RATE_LIMITED),
            ("Monthly usage limit reached", EXHAUSTED),
            ("Insufficient Balance", EXHAUSTED),
            ("Service Unavailable", UNAVAILABLE),
            ("token rate limit", RATE_LIMITED),
            ("upstream endpoint unavailable", UNAVAILABLE),
            ("pending requests", RATE_LIMITED),
            ("in-flight credits", RATE_LIMITED),
        ] {
            assert_eq!(classify_upstream_failure(429, body), expected, "{body}");
        }
        // Malformed JSON never invents an exhaustion; a truncated provider code
        // still classifies through the exact boundary-aware fallback.
        assert_eq!(
            classify_upstream_failure(429, r#"{"error":{"code":20"#),
            RATE_LIMITED
        );
        assert_eq!(classify_upstream_failure(429, r#"{"code":2056"#), EXHAUSTED);
        assert_eq!(classify_upstream_failure(400, "not json at all"), None);
        // A success body is never a failure, whatever it carries.
        assert_eq!(
            classify_upstream_failure(200, r#"{"error":{"code":2056,"message":"x"}}"#),
            None
        );
    }
}
