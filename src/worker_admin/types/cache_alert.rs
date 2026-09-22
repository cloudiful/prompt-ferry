use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const CACHE_ALERT_WINDOW_MINUTES_MIN: i32 = 5;
const CACHE_ALERT_WINDOW_MINUTES_MAX: i32 = 1440;
const CACHE_ALERT_MIN_TURNS_MIN: i32 = 2;
const CACHE_ALERT_MIN_TURNS_MAX: i32 = 100;
const CACHE_ALERT_COOLDOWN_MINUTES_MIN: i32 = 5;
const CACHE_ALERT_COOLDOWN_MINUTES_MAX: i32 = 1440;

fn default_cache_alert_window_minutes() -> i32 {
    30
}

fn default_cache_alert_min_turns() -> i32 {
    5
}

fn default_cache_alert_threshold() -> f64 {
    0.2
}

fn default_cache_alert_cooldown_minutes() -> i32 {
    60
}

/// Continuous-session cache alert policy. DingTalk is the only delivery
/// channel for now; `dingtalk_secret` is stored but never echoed back by the
/// admin API, and an empty value on write keeps the stored one.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct CacheAlertSettings {
    pub enabled: bool,
    #[serde(default = "default_cache_alert_window_minutes")]
    pub window_minutes: i32,
    #[serde(default = "default_cache_alert_min_turns")]
    pub min_turns: i32,
    #[serde(default = "default_cache_alert_threshold")]
    pub threshold: f64,
    #[serde(default = "default_cache_alert_cooldown_minutes")]
    pub cooldown_minutes: i32,
    #[serde(default)]
    pub dingtalk_webhook_url: String,
    #[serde(default)]
    pub dingtalk_secret: String,
}

impl Default for CacheAlertSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            window_minutes: default_cache_alert_window_minutes(),
            min_turns: default_cache_alert_min_turns(),
            threshold: default_cache_alert_threshold(),
            cooldown_minutes: default_cache_alert_cooldown_minutes(),
            dingtalk_webhook_url: String::new(),
            dingtalk_secret: String::new(),
        }
    }
}

impl CacheAlertSettings {
    /// Defensive clamp for values read from storage; the admin API rejects
    /// out-of-range input with `validate` instead of silently clamping it.
    pub fn normalized(mut self) -> Self {
        self.window_minutes = self.window_minutes.clamp(
            CACHE_ALERT_WINDOW_MINUTES_MIN,
            CACHE_ALERT_WINDOW_MINUTES_MAX,
        );
        self.min_turns = self
            .min_turns
            .clamp(CACHE_ALERT_MIN_TURNS_MIN, CACHE_ALERT_MIN_TURNS_MAX);
        self.threshold = if self.threshold.is_finite() {
            self.threshold.clamp(0.0, 1.0)
        } else {
            default_cache_alert_threshold()
        };
        self.cooldown_minutes = self.cooldown_minutes.clamp(
            CACHE_ALERT_COOLDOWN_MINUTES_MIN,
            CACHE_ALERT_COOLDOWN_MINUTES_MAX,
        );
        self.dingtalk_webhook_url = self.dingtalk_webhook_url.trim().to_string();
        self
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if !(CACHE_ALERT_WINDOW_MINUTES_MIN..=CACHE_ALERT_WINDOW_MINUTES_MAX)
            .contains(&self.window_minutes)
        {
            anyhow::bail!(
                "window_minutes must be between {CACHE_ALERT_WINDOW_MINUTES_MIN} and {CACHE_ALERT_WINDOW_MINUTES_MAX}"
            );
        }
        if !(CACHE_ALERT_MIN_TURNS_MIN..=CACHE_ALERT_MIN_TURNS_MAX).contains(&self.min_turns) {
            anyhow::bail!(
                "min_turns must be between {CACHE_ALERT_MIN_TURNS_MIN} and {CACHE_ALERT_MIN_TURNS_MAX}"
            );
        }
        if !self.threshold.is_finite() || !(0.0..=1.0).contains(&self.threshold) {
            anyhow::bail!("threshold must be between 0 and 1");
        }
        if !(CACHE_ALERT_COOLDOWN_MINUTES_MIN..=CACHE_ALERT_COOLDOWN_MINUTES_MAX)
            .contains(&self.cooldown_minutes)
        {
            anyhow::bail!(
                "cooldown_minutes must be between {CACHE_ALERT_COOLDOWN_MINUTES_MIN} and {CACHE_ALERT_COOLDOWN_MINUTES_MAX}"
            );
        }
        Ok(())
    }

    /// Admin API view: the stored DingTalk secret is write-only.
    pub fn redacted(&self) -> Self {
        Self {
            dingtalk_secret: String::new(),
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CacheAlertSettings;
    use serde_json::json;

    #[test]
    fn cache_alert_defaults_match_the_documented_rule() {
        let settings = CacheAlertSettings::default();

        assert!(!settings.enabled);
        assert_eq!(settings.window_minutes, 30);
        assert_eq!(settings.min_turns, 5);
        assert!((settings.threshold - 0.2).abs() < 1e-12);
        assert_eq!(settings.cooldown_minutes, 60);
        assert!(settings.dingtalk_webhook_url.is_empty());
    }

    #[test]
    fn cache_alert_deserializes_partial_payload_with_defaults() {
        let settings: CacheAlertSettings =
            serde_json::from_value(json!({ "enabled": true })).expect("partial payload");

        assert!(settings.enabled);
        assert_eq!(settings.window_minutes, 30);
        assert_eq!(settings.min_turns, 5);
        assert_eq!(settings.cooldown_minutes, 60);
    }

    #[test]
    fn cache_alert_normalizes_out_of_range_values_from_storage() {
        let normalized = CacheAlertSettings {
            window_minutes: 0,
            min_turns: 1,
            threshold: 9.0,
            cooldown_minutes: 10_000,
            ..CacheAlertSettings::default()
        }
        .normalized();

        assert_eq!(normalized.window_minutes, 5);
        assert_eq!(normalized.min_turns, 2);
        assert_eq!(normalized.threshold, 1.0);
        assert_eq!(normalized.cooldown_minutes, 1440);
    }

    #[test]
    fn cache_alert_rejects_out_of_range_requests_instead_of_clamping() {
        let invalid = CacheAlertSettings {
            threshold: 5.0,
            ..CacheAlertSettings::default()
        };
        assert!(invalid.validate().is_err());

        let invalid_window = CacheAlertSettings {
            window_minutes: 1,
            ..CacheAlertSettings::default()
        };
        assert!(invalid_window.validate().is_err());

        let valid = CacheAlertSettings {
            window_minutes: 30,
            threshold: 0.25,
            ..CacheAlertSettings::default()
        };
        assert!(valid.validate().is_ok());
    }

    #[test]
    fn cache_alert_redacted_view_hides_the_stored_secret() {
        let settings = CacheAlertSettings {
            dingtalk_secret: "secret".to_string(),
            ..CacheAlertSettings::default()
        };

        assert_eq!(settings.redacted().dingtalk_secret, "");
    }
}
