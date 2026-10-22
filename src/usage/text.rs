use super::*;

use std::fmt;
use std::marker::PhantomData;

use serde::de::value::MapAccessDeserializer;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

pub fn extract_usage(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("usage")?;
    let mut input_tokens = usage
        .get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(Value::as_i64)
        .map(non_negative);
    let output_tokens = usage
        .get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(Value::as_i64)
        .map(non_negative);
    let cached_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .or_else(|| usage.get("prompt_tokens_details"))
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_i64)
        })
        .map(non_negative);
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .or_else(|| usage.get("prompt_tokens_details"))
                .and_then(|details| {
                    details
                        .get("cache_read_tokens")
                        .or_else(|| details.get("cached_tokens"))
                })
                .and_then(Value::as_i64)
        })
        .map(non_negative);
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("input_tokens_details")
                .or_else(|| usage.get("prompt_tokens_details"))
                .and_then(|details| details.get("cache_write_tokens"))
                .and_then(Value::as_i64)
        })
        .map(non_negative);
    // Only the native Anthropic shape treats the top-level cache meters as
    // additive to `input_tokens`/`prompt_tokens`. That shape exposes
    // `cache_read_input_tokens`/`cache_creation_input_tokens`, never a
    // `prompt_tokens_details`/`input_tokens_details` block, and never reports an
    // explicit `total_tokens`.
    //
    // OpenAI chat/responses and DeepSeek already count the cached tokens inside
    // `input_tokens`/`prompt_tokens` (cache is a subset). The adapters
    // (anthropic_compat / anthropic_stream_adapter) emit pre-folded `input_tokens`
    // alongside `input_tokens_details`; raw passthrough may omit the details
    // block but still reports an explicit total that equals input+output, which
    // proves the input already includes the cache. Folding would double-count in
    // every one of these cases.
    let has_details =
        usage.get("input_tokens_details").is_some() || usage.get("prompt_tokens_details").is_some();
    let explicit_total_matches = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .map(non_negative)
        .zip(input_tokens)
        .zip(output_tokens)
        .is_some_and(|((total, input), output)| total == input + output);
    let anthropic_top_level_cache = usage.get("cache_read_input_tokens").is_some()
        || usage.get("cache_creation_input_tokens").is_some();
    if anthropic_top_level_cache && !has_details && !explicit_total_matches {
        let cache_total =
            cache_read_tokens.unwrap_or_default() + cache_write_tokens.unwrap_or_default();
        input_tokens = input_tokens
            .map(|value| value.saturating_add(cache_total))
            .or((cache_total > 0).then_some(cache_total));
    }
    let total_tokens = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .map(non_negative)
        .or_else(|| {
            input_tokens
                .zip(output_tokens)
                .map(|(input, output)| input + output)
        });

    if input_tokens.is_none()
        && output_tokens.is_none()
        && total_tokens.is_none()
        && cached_tokens.is_none()
        && cache_read_tokens.is_none()
        && cache_write_tokens.is_none()
    {
        return None;
    }

    Some(TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens,
        cached_tokens,
        cache_read_tokens,
        cache_write_tokens,
    })
}

/// Issue #1032: the single embedding usage normalization every entry point
/// shares.
///
/// An embeddings response reports `prompt_tokens`/`input_tokens` and, at most,
/// an explicit `total_tokens`; there is no output. This never derives a total
/// from an output/completion field, never carries an upstream completion/output
/// value into a billable meter, and never invents input or cache usage. With a
/// reported input the output is zero and an absent total falls back to the
/// actual input; without one only an explicitly reported total survives, as
/// factual metadata.
fn normalize_embedding_usage(
    input_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
) -> Option<TokenUsage> {
    let Some(input) = input_tokens.map(non_negative) else {
        return total_tokens.map(non_negative).map(|total| TokenUsage {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(total),
            cached_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
        });
    };
    Some(TokenUsage {
        input_tokens: Some(input),
        output_tokens: Some(0),
        total_tokens: total_tokens.map(non_negative).or(Some(input)),
        cached_tokens: cached_tokens.map(non_negative),
        cache_read_tokens: cache_read_tokens.map(non_negative),
        cache_write_tokens: cache_write_tokens.map(non_negative),
    })
}

/// Issue #1032: embedding usage read from an already-materialized value. This
/// keeps the historical value-based semantics; the response body path uses the
/// typed envelope below, and both delegate to the same normalization.
pub fn extract_embedding_usage(usage: &Value) -> Option<TokenUsage> {
    let (cached_tokens, cache_read_tokens, cache_write_tokens) = embedding_cache_fields(usage);
    normalize_embedding_usage(
        usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens"))
            .and_then(Value::as_i64),
        usage.get("total_tokens").and_then(Value::as_i64),
        cached_tokens,
        cache_read_tokens,
        cache_write_tokens,
    )
}

/// Issue #1032: metadata read from a complete embeddings response body.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EmbeddingBodyMetadata {
    pub model: Option<String>,
    pub id: Option<String>,
    pub conversation: Option<String>,
    pub usage: Option<TokenUsage>,
}

/// A metadata key that distinguishes an absent key from a key that is present
/// but `null`, wrong-typed, or otherwise unusable. A present-but-unusable field
/// is never replaced by a later unrelated object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataField<T> {
    Missing,
    Present(Option<T>),
}

impl<T> Default for MetadataField<T> {
    fn default() -> Self {
        Self::Missing
    }
}

impl<T> MetadataField<T> {
    fn value(&self) -> Option<&T> {
        match self {
            Self::Present(Some(value)) => Some(value),
            _ => None,
        }
    }
}

/// The `usage` shape the embeddings semantics read. Unknown fields (including
/// any vector extension) are skipped structurally by `serde`, never
/// materialized.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EmbeddingUsageFields {
    #[serde(deserialize_with = "lenient_i64")]
    input_tokens: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    prompt_tokens: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    total_tokens: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    cache_read_input_tokens: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    cache_creation_input_tokens: Option<i64>,
    input_tokens_details: MetadataField<EmbeddingCacheDetails>,
    prompt_tokens_details: MetadataField<EmbeddingCacheDetails>,
}

impl EmbeddingUsageFields {
    fn normalize(&self) -> Option<TokenUsage> {
        let details = self
            .input_tokens_details
            .value()
            .or_else(|| self.prompt_tokens_details.value());
        normalize_embedding_usage(
            self.input_tokens.or(self.prompt_tokens),
            self.total_tokens,
            self.cache_read_input_tokens
                .or_else(|| details.and_then(|details| details.cached_tokens)),
            self.cache_read_input_tokens.or_else(|| {
                details.and_then(|details| details.cache_read_tokens.or(details.cached_tokens))
            }),
            self.cache_creation_input_tokens
                .or_else(|| details.and_then(|details| details.cache_write_tokens)),
        )
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EmbeddingCacheDetails {
    #[serde(deserialize_with = "lenient_i64")]
    cached_tokens: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    cache_read_tokens: Option<i64>,
    #[serde(deserialize_with = "lenient_i64")]
    cache_write_tokens: Option<i64>,
}

/// One-level metadata, shared by the root payload and a `response`/`message`
/// wrapper. Any other field of the object is skipped structurally.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EmbeddingPayload {
    model: MetadataField<String>,
    id: MetadataField<String>,
    conversation: MetadataField<String>,
    usage: MetadataField<EmbeddingUsageFields>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EmbeddingEnvelope {
    model: MetadataField<String>,
    id: MetadataField<String>,
    conversation: MetadataField<String>,
    usage: MetadataField<EmbeddingUsageFields>,
    response: MetadataField<EmbeddingPayload>,
    message: MetadataField<EmbeddingPayload>,
}

/// Issue #1032: extract the embeddings accounting metadata from the complete
/// accepted response bytes with one typed, metadata-only deserialization.
///
/// The body is borrowed and never copied; the `data` vectors and every other
/// unknown field are skipped by `serde` without being materialized. A malformed
/// or truncated body yields no metadata. The payload keeps the historical
/// provenance: `root.response` when that key is present, else `root.message`
/// when present, else the root itself; a key selected null or wrong-typed stays
/// unusable instead of falling through to an unrelated object.
pub fn extract_embedding_body_metadata(body: &[u8]) -> EmbeddingBodyMetadata {
    let Ok(envelope) = serde_json::from_slice::<EmbeddingEnvelope>(body) else {
        return EmbeddingBodyMetadata::default();
    };
    let empty = EmbeddingPayload::default();
    let payload = match &envelope.response {
        MetadataField::Missing => &envelope.message,
        present => present,
    };
    let payload = match payload {
        MetadataField::Present(Some(payload)) => payload,
        _ => &empty,
    };
    EmbeddingBodyMetadata {
        model: select(&payload.model, &envelope.model)
            .value()
            .map(|value| value.to_string()),
        id: select(&payload.id, &envelope.id)
            .value()
            .map(|value| value.to_string()),
        conversation: select(&payload.conversation, &envelope.conversation)
            .value()
            .map(|value| value.to_string()),
        usage: select(&payload.usage, &envelope.usage)
            .value()
            .and_then(EmbeddingUsageFields::normalize),
    }
}

/// The payload field when it is present, otherwise the root field.
fn select<'a, T>(
    payload: &'a MetadataField<T>,
    root: &'a MetadataField<T>,
) -> &'a MetadataField<T> {
    match payload {
        MetadataField::Missing => root,
        present => present,
    }
}

/// A string or metadata object read leniently: any other JSON shape is present
/// but unusable, so the document keeps parsing and unrelated fields are not
/// consulted.
trait LenientMetadata<'de>: Sized {
    fn from_string(value: &str) -> Option<Self>;
    fn from_map<A>(map: A) -> Result<Option<Self>, A::Error>
    where
        A: MapAccess<'de>;
}

impl<'de> LenientMetadata<'de> for String {
    fn from_string(value: &str) -> Option<Self> {
        Some(value.to_string())
    }

    fn from_map<A>(mut map: A) -> Result<Option<Self>, A::Error>
    where
        A: MapAccess<'de>,
    {
        // A wrong-typed string metadata value must be consumed structurally so
        // the rest of the object (including a valid sibling `usage`) still
        // parses. The consumed values are discarded, never materialized.
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }
}

impl<'de> LenientMetadata<'de> for EmbeddingPayload {
    fn from_string(_value: &str) -> Option<Self> {
        None
    }

    fn from_map<A>(map: A) -> Result<Option<Self>, A::Error>
    where
        A: MapAccess<'de>,
    {
        EmbeddingPayload::deserialize(MapAccessDeserializer::new(map)).map(Some)
    }
}

impl<'de> LenientMetadata<'de> for EmbeddingUsageFields {
    fn from_string(_value: &str) -> Option<Self> {
        None
    }

    fn from_map<A>(map: A) -> Result<Option<Self>, A::Error>
    where
        A: MapAccess<'de>,
    {
        EmbeddingUsageFields::deserialize(MapAccessDeserializer::new(map)).map(Some)
    }
}

impl<'de> LenientMetadata<'de> for EmbeddingCacheDetails {
    fn from_string(_value: &str) -> Option<Self> {
        None
    }

    fn from_map<A>(map: A) -> Result<Option<Self>, A::Error>
    where
        A: MapAccess<'de>,
    {
        EmbeddingCacheDetails::deserialize(MapAccessDeserializer::new(map)).map(Some)
    }
}

impl<'de, T: LenientMetadata<'de>> Deserialize<'de> for MetadataField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MetadataVisitor<T>(PhantomData<T>);

        impl<'de, T: LenientMetadata<'de>> Visitor<'de> for MetadataVisitor<T> {
            type Value = MetadataField<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an optional metadata field")
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                T::from_map(map).map(MetadataField::Present)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(T::from_string(value)))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(T::from_string(&value)))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(None))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(None))
            }

            fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(None))
            }

            fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(None))
            }

            fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(None))
            }

            fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(MetadataField::Present(None))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(MetadataField::Present(None))
            }
        }

        deserializer.deserialize_any(MetadataVisitor(PhantomData))
    }
}

/// Read an integer leniently: a wrong-typed or `null` value is unusable rather
/// than a hard parse error.
fn lenient_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct LenientI64;

    impl<'de> Visitor<'de> for LenientI64 {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an optional integer")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value.min(i64::MAX as u64) as i64))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_string<E>(self, _value: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
            Ok(None)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }

    deserializer.deserialize_any(LenientI64)
}

/// Cache meters for an embedding response. They are recognized only when the
/// response actually reports an input, mirroring the plain OpenAI-compatible
/// field shapes rather than the chat extractor's Anthropic folding.
fn embedding_cache_fields(usage: &Value) -> (Option<i64>, Option<i64>, Option<i64>) {
    let details = usage
        .get("input_tokens_details")
        .or_else(|| usage.get("prompt_tokens_details"));
    let cached_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            details
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_i64)
        })
        .map(non_negative);
    let cache_read_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            details
                .and_then(|details| {
                    details
                        .get("cache_read_tokens")
                        .or_else(|| details.get("cached_tokens"))
                })
                .and_then(Value::as_i64)
        })
        .map(non_negative);
    let cache_write_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            details
                .and_then(|details| details.get("cache_write_tokens"))
                .and_then(Value::as_i64)
        })
        .map(non_negative);
    (cached_tokens, cache_read_tokens, cache_write_tokens)
}

fn non_negative(value: i64) -> i64 {
    value.max(0)
}

pub(super) fn extract_output_text(value: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(text) = value
        .get("delta")
        .filter(|_| is_visible_delta_event(value))
        .or_else(|| value.get("text"))
        .or_else(|| value.get("output_text"))
        .and_then(Value::as_str)
    {
        parts.push(text.to_string());
    }
    if let Some(text) = value
        .get("delta")
        .and_then(|delta| delta.get("text").or_else(|| delta.get("thinking")))
        .and_then(Value::as_str)
    {
        parts.push(text.to_string());
    }
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(content) = choice
                .get("delta")
                .or_else(|| choice.get("message"))
                .and_then(|message| message.get("content"))
            {
                let text = value_text(content);
                if !text.is_empty() {
                    parts.push(text);
                }
            }
        }
    }
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        for item in output {
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for part in content {
                    let text = value_text(
                        part.get("text")
                            .or_else(|| part.get("output_text"))
                            .unwrap_or(part),
                    );
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
            }
        }
    }
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        for part in content {
            let text = value_text(
                part.get("text")
                    .or_else(|| part.get("thinking"))
                    .unwrap_or(part),
            );
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join("")
}

pub(super) fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(value_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .or_else(|| object.get("content"))
            .or_else(|| object.get("input_text"))
            .or_else(|| object.get("output_text"))
            .map(value_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub(super) fn append_text(target: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    target.push_str(text);
}

pub fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut value = text.chars().take(limit).collect::<String>();
    value.push('…');
    value
}

fn is_visible_delta_event(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|event_type| event_type == "response.output_text.delta")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::extract_usage;

    #[test]
    fn extracts_openai_chat_usage_and_cache_read_tokens() {
        let usage = extract_usage(&json!({
            "usage": {
                "prompt_tokens": 120,
                "completion_tokens": 20,
                "total_tokens": 140,
                "prompt_tokens_details": { "cached_tokens": 30 }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, None);
    }

    #[test]
    fn extracts_openai_responses_usage_and_separate_cache_meters() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 120,
                "output_tokens": 20,
                "input_tokens_details": {
                    "cached_tokens": 30,
                    "cache_read_tokens": 30,
                    "cache_write_tokens": 7
                }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, Some(7));
    }

    #[test]
    fn returns_unknown_when_provider_has_no_usage_fields() {
        assert!(extract_usage(&json!({ "usage": {} })).is_none());
        assert!(extract_usage(&json!({})).is_none());
    }

    #[test]
    fn extracts_anthropic_cache_usage() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 120,
                "output_tokens": 20,
                "cache_read_input_tokens": 30,
                "cache_creation_input_tokens": 7
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(157));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cached_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, Some(7));
    }

    #[test]
    fn folds_anthropic_cache_when_input_tokens_is_absent() {
        let usage = extract_usage(&json!({
            "usage": {
                "output_tokens": 12,
                "cache_read_input_tokens": 30,
                "cache_creation_input_tokens": 7
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(37));
        assert_eq!(usage.output_tokens, Some(12));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cache_write_tokens, Some(7));
    }

    #[test]
    fn keeps_anthropic_input_when_cache_fields_are_zero() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.cache_read_tokens, Some(0));
        assert_eq!(usage.cache_write_tokens, Some(0));
    }

    #[test]
    fn clamps_negative_cache_and_input_values_to_zero() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 120,
                "output_tokens": -5,
                "cache_read_input_tokens": -30,
                "cache_creation_input_tokens": -7
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(0));
        assert_eq!(usage.cache_read_tokens, Some(0));
        assert_eq!(usage.cache_write_tokens, Some(0));
        assert_eq!(usage.total_tokens, Some(120));
    }

    #[test]
    fn preserves_explicit_total_tokens_over_computed_value() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "total_tokens": 9999,
                "cache_read_input_tokens": 30,
                "cache_creation_input_tokens": 5
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(135));
        assert_eq!(usage.total_tokens, Some(9999));
    }

    #[test]
    fn prefers_nested_cache_read_tokens_over_inconsistent_cached_tokens() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "input_tokens_details": {
                    "cached_tokens": 99,
                    "cache_read_tokens": 30
                }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.cache_read_tokens, Some(30));
        assert_eq!(usage.cached_tokens, Some(99));
    }

    #[test]
    fn openai_chat_with_only_cached_tokens_in_details() {
        let usage = extract_usage(&json!({
            "usage": {
                "prompt_tokens": 80,
                "completion_tokens": 20,
                "prompt_tokens_details": { "cached_tokens": 40 }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(80));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.cache_read_tokens, Some(40));
        assert_eq!(usage.cache_write_tokens, None);
        assert_eq!(usage.total_tokens, Some(100));
    }

    #[test]
    fn cache_total_exceeding_input_does_not_inflate_canonical_input() {
        let usage = extract_usage(&json!({
            "usage": {
                "input_tokens": 50,
                "output_tokens": 10,
                "cache_read_input_tokens": 80,
                "cache_creation_input_tokens": 0
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(130));
        assert_eq!(usage.cache_read_tokens, Some(80));
    }

    #[test]
    fn raw_passthrough_with_explicit_total_does_not_fold_input() {
        // OpenAI/DeepSeek raw passthrough may omit the details block but still
        // reports an explicit total equal to input+output, which proves the
        // input already includes the cache. Folding would double-count.
        let usage = extract_usage(&json!({
            "usage": {
                "prompt_tokens": 27581,
                "completion_tokens": 130,
                "total_tokens": 27711,
                "cache_read_input_tokens": 27520
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(27581));
        assert_eq!(usage.output_tokens, Some(130));
        assert_eq!(usage.total_tokens, Some(27711));
        assert_eq!(usage.cache_read_tokens, Some(27520));
    }

    #[test]
    fn openai_chat_prompt_tokens_with_details_does_not_fold_input() {
        // OpenAI chat reports the full prompt_tokens plus a top-level cache meter
        // and a prompt_tokens_details block. prompt_tokens already counts the
        // cache, so it must not be folded.
        let usage = extract_usage(&json!({
            "usage": {
                "prompt_tokens": 8868,
                "completion_tokens": 5499,
                "cache_read_input_tokens": 4534,
                "prompt_tokens_details": {
                    "cached_tokens": 4534
                }
            }
        }))
        .unwrap();

        assert_eq!(usage.input_tokens, Some(8868));
        assert_eq!(usage.output_tokens, Some(5499));
        assert_eq!(usage.cache_read_tokens, Some(4534));
    }
}
