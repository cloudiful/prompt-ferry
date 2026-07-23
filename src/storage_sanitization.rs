use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizationStats {
    pub nul_count: usize,
}

impl SanitizationStats {
    pub fn merge(&mut self, other: Self) {
        self.nul_count += other.nul_count;
    }

    pub fn sanitized(self) -> bool {
        self.nul_count > 0
    }
}

pub fn sanitize_text_for_storage(text: &str) -> (String, SanitizationStats) {
    let nul_count = text.chars().filter(|ch| *ch == '\0').count();
    if nul_count == 0 {
        return (text.to_string(), SanitizationStats::default());
    }
    let sanitized = text.chars().filter(|ch| *ch != '\0').collect();
    (sanitized, SanitizationStats { nul_count })
}

pub fn sanitize_optional_text_for_storage(
    text: Option<String>,
) -> (Option<String>, SanitizationStats) {
    let Some(text) = text else {
        return (None, SanitizationStats::default());
    };
    let (sanitized, stats) = sanitize_text_for_storage(&text);
    (Some(sanitized), stats)
}

pub fn sanitize_json_for_storage(value: &Value) -> (Value, SanitizationStats) {
    match value {
        Value::String(text) => {
            let (sanitized, stats) = sanitize_text_for_storage(text);
            (Value::String(sanitized), stats)
        }
        Value::Array(items) => {
            let mut stats = SanitizationStats::default();
            let sanitized = items
                .iter()
                .map(|item| {
                    let (item, item_stats) = sanitize_json_for_storage(item);
                    stats.merge(item_stats);
                    item
                })
                .collect();
            (Value::Array(sanitized), stats)
        }
        Value::Object(object) => {
            let mut stats = SanitizationStats::default();
            let sanitized = object
                .iter()
                .map(|(key, value)| {
                    let (value, value_stats) = sanitize_json_for_storage(value);
                    stats.merge(value_stats);
                    (key.clone(), value)
                })
                .collect();
            (Value::Object(sanitized), stats)
        }
        _ => (value.clone(), SanitizationStats::default()),
    }
}

pub fn sanitize_optional_json_for_storage(
    value: Option<Value>,
) -> (Option<Value>, SanitizationStats) {
    let Some(value) = value else {
        return (None, SanitizationStats::default());
    };
    let (sanitized, stats) = sanitize_json_for_storage(&value);
    (Some(sanitized), stats)
}

#[cfg(test)]
mod tests {
    use super::{
        SanitizationStats, sanitize_json_for_storage, sanitize_optional_text_for_storage,
        sanitize_text_for_storage,
    };

    #[test]
    fn removes_only_nul_from_text() {
        let (sanitized, stats) = sanitize_text_for_storage("a\0b\n\t\r");
        assert_eq!(sanitized, "ab\n\t\r");
        assert_eq!(stats, SanitizationStats { nul_count: 1 });
    }

    #[test]
    fn optional_text_passthrough_none() {
        let (sanitized, stats) = sanitize_optional_text_for_storage(None);
        assert_eq!(sanitized, None);
        assert_eq!(stats, SanitizationStats::default());
    }

    #[test]
    fn recursively_sanitizes_json_strings() {
        let value = serde_json::json!({
            "outer": "\0value",
            "items": ["a", "b\0", {"nested": "c\0d"}],
            "count": 3
        });
        let (sanitized, stats) = sanitize_json_for_storage(&value);

        assert_eq!(
            sanitized,
            serde_json::json!({
                "outer": "value",
                "items": ["a", "b", {"nested": "cd"}],
                "count": 3
            })
        );
        assert_eq!(stats, SanitizationStats { nul_count: 3 });
    }
}
