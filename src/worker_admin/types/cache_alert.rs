use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

const CACHE_ALERT_WINDOW_MINUTES_MIN: i32 = 5;
const CACHE_ALERT_WINDOW_MINUTES_MAX: i32 = 1440;
const CACHE_ALERT_MIN_TURNS_MIN: i32 = 2;
const CACHE_ALERT_MIN_TURNS_MAX: i32 = 100;
const CACHE_ALERT_COOLDOWN_MINUTES_MIN: i32 = 5;
const CACHE_ALERT_COOLDOWN_MINUTES_MAX: i32 = 1440;

const CACHE_ALERT_SMTP_PORT_MIN: u16 = 1;
const CACHE_ALERT_SMTP_PORT_MAX: u16 = 65535;

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

fn default_cache_alert_smtp_port() -> u16 {
    465
}

/// Transport security for the SMTP channel. Only encrypted transports are
/// exposed: an operator can pick implicit TLS or STARTTLS, never plaintext.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheAlertSmtpTls {
    ImplicitTls,
    StartTls,
}

/// SMTP delivery configuration for the continuous-session cache alert.
///
/// `password` is stored like the DingTalk secret: persisted as written, never
/// echoed by the admin API, and an empty value on write keeps the stored one.
#[derive(Clone, Deserialize, Serialize, ToSchema, PartialEq)]
#[serde(default)]
pub struct CacheAlertSmtpSettings {
    pub enabled: bool,
    pub host: String,
    #[serde(default = "default_cache_alert_smtp_port")]
    pub port: u16,
    pub tls: CacheAlertSmtpTls,
    pub from: String,
    pub to: Vec<String>,
    pub username: String,
    pub password: String,
}

impl Default for CacheAlertSmtpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: default_cache_alert_smtp_port(),
            tls: CacheAlertSmtpTls::ImplicitTls,
            from: String::new(),
            to: Vec::new(),
            username: String::new(),
            password: String::new(),
        }
    }
}

impl std::fmt::Debug for CacheAlertSmtpSettings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Username, sender, recipients, and the password are credential or
        // personal data; a stray `{:?}` must not reveal them.
        formatter
            .debug_struct("CacheAlertSmtpSettings")
            .field("enabled", &self.enabled)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("from", &presence(&self.from))
            .field("to", &format!("<{} recipient(s)>", self.to.len()))
            .field("username", &presence(&self.username))
            .field("password", &presence(&self.password))
            .finish()
    }
}

impl CacheAlertSmtpSettings {
    fn normalized(mut self) -> Self {
        self.host = self.host.trim().to_string();
        self.from = self.from.trim().to_string();
        self.to = self
            .to
            .iter()
            .map(|recipient| recipient.trim().to_string())
            .filter(|recipient| !recipient.is_empty())
            .collect();
        self.username = self.username.trim().to_string();
        self
    }

    fn validate(&self) -> anyhow::Result<()> {
        if !(CACHE_ALERT_SMTP_PORT_MIN..=CACHE_ALERT_SMTP_PORT_MAX).contains(&self.port) {
            anyhow::bail!(
                "smtp.port must be between {CACHE_ALERT_SMTP_PORT_MIN} and {CACHE_ALERT_SMTP_PORT_MAX}"
            );
        }
        let has_username = !self.username.trim().is_empty();
        let has_password = !self.password.trim().is_empty();
        if has_username != has_password {
            anyhow::bail!("smtp.username and smtp.password must be set together");
        }
        if !self.host.trim().is_empty() && !valid_smtp_host(&self.host) {
            anyhow::bail!("smtp.host must be a host name or address without a scheme");
        }
        if !self.from.trim().is_empty() && !valid_email_address(&self.from) {
            anyhow::bail!("smtp.from must be a valid email address");
        }
        if !self
            .to
            .iter()
            .all(|recipient| valid_email_address(recipient))
        {
            anyhow::bail!("smtp.to must contain only valid email addresses");
        }
        if self.enabled {
            if self.host.trim().is_empty() {
                anyhow::bail!("smtp.host is required when the SMTP channel is enabled");
            }
            if self.from.trim().is_empty() {
                anyhow::bail!("smtp.from is required when the SMTP channel is enabled");
            }
            if self.to.is_empty() {
                anyhow::bail!("smtp.to must contain at least one email address");
            }
        }
        Ok(())
    }

    fn redacted(&self) -> Self {
        Self {
            password: String::new(),
            ..self.clone()
        }
    }

    fn has_password(&self) -> bool {
        !self.password.trim().is_empty()
    }
}

/// Continuous-session cache alert policy. DingTalk and SMTP are independent
/// delivery channels; `dingtalk_secret` and `smtp.password` are stored but never
/// echoed back by the admin API, and an empty value on write keeps the stored
/// one.
#[derive(Clone, Deserialize, Serialize, ToSchema, PartialEq)]
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
    /// Persisted SMTP configuration. It is part of the stored JSON and the
    /// write request, but the legacy `CacheAlertSettings` schema stays the
    /// published component for old clients; the admin write contract is
    /// [`CacheAlertSettingsRequest`].
    #[schema(ignore)]
    pub smtp: CacheAlertSmtpSettings,
}

/// Write request for the cache alert policy.
///
/// `smtp` is optional on purpose: an older client that omits it keeps the
/// stored SMTP configuration, while an explicit object (including
/// `enabled: false`) replaces it. A blank SMTP password keeps the stored one.
#[derive(Clone, Deserialize, ToSchema)]
#[serde(default)]
pub struct CacheAlertSettingsRequest {
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
    pub smtp: Option<CacheAlertSmtpSettings>,
}

impl Default for CacheAlertSettingsRequest {
    fn default() -> Self {
        Self {
            enabled: false,
            window_minutes: default_cache_alert_window_minutes(),
            min_turns: default_cache_alert_min_turns(),
            threshold: default_cache_alert_threshold(),
            cooldown_minutes: default_cache_alert_cooldown_minutes(),
            dingtalk_webhook_url: String::new(),
            dingtalk_secret: String::new(),
            smtp: None,
        }
    }
}

/// Admin API view of the SMTP channel: the password is always blanked and
/// `has_smtp_password` reports whether one is stored.
#[derive(Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct CacheAlertSmtpSettingsResponse {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub tls: CacheAlertSmtpTls,
    pub from: String,
    pub to: Vec<String>,
    pub username: String,
    pub password: String,
    pub has_smtp_password: bool,
}

impl std::fmt::Debug for CacheAlertSmtpSettingsResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CacheAlertSmtpSettingsResponse")
            .field("enabled", &self.enabled)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("from", &presence(&self.from))
            .field("to", &format!("<{} recipient(s)>", self.to.len()))
            .field("username", &presence(&self.username))
            .field("password", &presence(&self.password))
            .field("has_smtp_password", &self.has_smtp_password)
            .finish()
    }
}

/// Admin API view of [`CacheAlertSettings`]: the write-only secrets are always
/// blanked, and `has_dingtalk_secret` / `smtp.has_smtp_password` report whether
/// a non-blank secret is actually stored. The request schema and the persisted
/// JSON never carry these derived flags.
#[derive(Clone, Deserialize, Serialize, ToSchema, PartialEq)]
pub struct CacheAlertSettingsResponse {
    pub enabled: bool,
    pub window_minutes: i32,
    pub min_turns: i32,
    pub threshold: f64,
    pub cooldown_minutes: i32,
    pub dingtalk_webhook_url: String,
    pub dingtalk_secret: String,
    pub has_dingtalk_secret: bool,
    /// Optional in the schema so a legacy client that never read it keeps
    /// compiling; the server always sends it.
    pub smtp: Option<CacheAlertSmtpSettingsResponse>,
}

impl std::fmt::Debug for CacheAlertSettings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CacheAlertSettings")
            .field("enabled", &self.enabled)
            .field("window_minutes", &self.window_minutes)
            .field("min_turns", &self.min_turns)
            .field("threshold", &self.threshold)
            .field("cooldown_minutes", &self.cooldown_minutes)
            .field(
                "dingtalk_webhook_url",
                &presence(&self.dingtalk_webhook_url),
            )
            .field("dingtalk_secret", &presence(&self.dingtalk_secret))
            .field("smtp", &self.smtp)
            .finish()
    }
}

impl std::fmt::Debug for CacheAlertSettingsRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CacheAlertSettingsRequest")
            .field("enabled", &self.enabled)
            .field("window_minutes", &self.window_minutes)
            .field("min_turns", &self.min_turns)
            .field("threshold", &self.threshold)
            .field("cooldown_minutes", &self.cooldown_minutes)
            .field(
                "dingtalk_webhook_url",
                &presence(&self.dingtalk_webhook_url),
            )
            .field("dingtalk_secret", &presence(&self.dingtalk_secret))
            .field("smtp", &self.smtp)
            .finish()
    }
}

impl std::fmt::Debug for CacheAlertSettingsResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CacheAlertSettingsResponse")
            .field("enabled", &self.enabled)
            .field("window_minutes", &self.window_minutes)
            .field("min_turns", &self.min_turns)
            .field("threshold", &self.threshold)
            .field("cooldown_minutes", &self.cooldown_minutes)
            .field(
                "dingtalk_webhook_url",
                &presence(&self.dingtalk_webhook_url),
            )
            .field("dingtalk_secret", &presence(&self.dingtalk_secret))
            .field("has_dingtalk_secret", &self.has_dingtalk_secret)
            .field("smtp", &self.smtp)
            .finish()
    }
}

fn presence(value: &str) -> &'static str {
    if value.trim().is_empty() {
        "<unset>"
    } else {
        "<redacted>"
    }
}

fn valid_smtp_host(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 253
        && !value.contains("://")
        && !value.contains('/')
        && !value.contains('@')
        && value
            .chars()
            .all(|character| !character.is_whitespace() && !character.is_control())
}

fn valid_email_address(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 254 {
        return false;
    }
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return false;
    }
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    if local.is_empty() || local.len() > 64 || local.contains('@') {
        return false;
    }
    if domain.is_empty()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
    {
        return false;
    }
    domain
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
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
            smtp: CacheAlertSmtpSettings::default(),
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
        self.smtp = self.smtp.normalized();
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
        self.smtp.validate()
    }

    /// Admin API view: both stored secrets are write-only.
    pub fn redacted(&self) -> Self {
        Self {
            dingtalk_secret: String::new(),
            smtp: self.smtp.redacted(),
            ..self.clone()
        }
    }

    /// Whether a non-blank DingTalk secret is currently stored; the secret
    /// value itself stays write-only.
    pub fn has_dingtalk_secret(&self) -> bool {
        !self.dingtalk_secret.trim().is_empty()
    }

    /// Whether a non-blank SMTP password is currently stored.
    pub fn has_smtp_password(&self) -> bool {
        self.smtp.has_password()
    }

    /// Redacted admin API response with the derived secret-presence flags.
    pub fn redacted_response(&self) -> CacheAlertSettingsResponse {
        let redacted = self.redacted();
        CacheAlertSettingsResponse {
            enabled: redacted.enabled,
            window_minutes: redacted.window_minutes,
            min_turns: redacted.min_turns,
            threshold: redacted.threshold,
            cooldown_minutes: redacted.cooldown_minutes,
            dingtalk_webhook_url: redacted.dingtalk_webhook_url,
            dingtalk_secret: redacted.dingtalk_secret,
            has_dingtalk_secret: self.has_dingtalk_secret(),
            smtp: Some(CacheAlertSmtpSettingsResponse {
                enabled: redacted.smtp.enabled,
                host: redacted.smtp.host,
                port: redacted.smtp.port,
                tls: redacted.smtp.tls,
                from: redacted.smtp.from,
                to: redacted.smtp.to,
                username: redacted.smtp.username,
                password: String::new(),
                has_smtp_password: self.has_smtp_password(),
            }),
        }
    }
}

impl CacheAlertSettingsRequest {
    /// Merge this write request over the stored policy.
    ///
    /// An omitted `smtp` object keeps the stored SMTP configuration, an
    /// explicit object replaces it, and a blank DingTalk secret or SMTP
    /// password keeps the stored credential.
    pub fn into_settings(self, stored: &CacheAlertSettings) -> CacheAlertSettings {
        let dingtalk_secret = if self.dingtalk_secret.trim().is_empty() {
            stored.dingtalk_secret.clone()
        } else {
            self.dingtalk_secret.trim().to_string()
        };
        let smtp = match self.smtp {
            Some(smtp) => {
                let requested_username = smtp.username.trim();
                // A blank password keeps the stored one only while the request
                // keeps the same username. Clearing or changing the username
                // without supplying a password drops the old credential instead
                // of pairing it with a different account.
                let password = if smtp.password.trim().is_empty() {
                    if requested_username == stored.smtp.username.trim() {
                        stored.smtp.password.clone()
                    } else {
                        String::new()
                    }
                } else {
                    smtp.password.trim().to_string()
                };
                CacheAlertSmtpSettings { password, ..smtp }
            }
            None => stored.smtp.clone(),
        };
        CacheAlertSettings {
            enabled: self.enabled,
            window_minutes: self.window_minutes,
            min_turns: self.min_turns,
            threshold: self.threshold,
            cooldown_minutes: self.cooldown_minutes,
            dingtalk_webhook_url: self.dingtalk_webhook_url,
            dingtalk_secret,
            smtp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CacheAlertSettings, CacheAlertSettingsRequest, CacheAlertSmtpSettings, CacheAlertSmtpTls,
    };
    use serde_json::json;

    fn smtp(enabled: bool) -> CacheAlertSmtpSettings {
        CacheAlertSmtpSettings {
            enabled,
            host: "smtp.example.com".to_string(),
            port: 587,
            tls: CacheAlertSmtpTls::StartTls,
            from: "alerts@example.com".to_string(),
            to: vec!["ops@example.com".to_string()],
            username: "alert-user".to_string(),
            password: "SEC-smtp-password".to_string(),
        }
    }

    #[test]
    fn cache_alert_defaults_match_the_documented_rule() {
        let settings = CacheAlertSettings::default();

        assert!(!settings.enabled);
        assert_eq!(settings.window_minutes, 30);
        assert_eq!(settings.min_turns, 5);
        assert!((settings.threshold - 0.2).abs() < 1e-12);
        assert_eq!(settings.cooldown_minutes, 60);
        assert!(settings.dingtalk_webhook_url.is_empty());
        assert!(!settings.smtp.enabled);
        assert_eq!(settings.smtp.port, 465);
        assert_eq!(settings.smtp.tls, CacheAlertSmtpTls::ImplicitTls);
        assert!(settings.smtp.to.is_empty());
    }

    #[test]
    fn cache_alert_deserializes_partial_payload_with_defaults() {
        let settings: CacheAlertSettings =
            serde_json::from_value(json!({ "enabled": true })).expect("partial payload");

        assert!(settings.enabled);
        assert_eq!(settings.window_minutes, 30);
        assert_eq!(settings.min_turns, 5);
        assert_eq!(settings.cooldown_minutes, 60);
        assert!(!settings.smtp.enabled);
    }

    #[test]
    fn legacy_stored_json_still_reads_and_keeps_smtp_disabled() {
        let settings: CacheAlertSettings = serde_json::from_value(json!({
            "enabled": true,
            "window_minutes": 30,
            "min_turns": 5,
            "threshold": 0.2,
            "cooldown_minutes": 60,
            "dingtalk_webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=x",
            "dingtalk_secret": "SEC-dingtalk"
        }))
        .expect("legacy stored settings");

        assert!(settings.enabled);
        assert_eq!(settings.dingtalk_secret, "SEC-dingtalk");
        assert!(!settings.smtp.enabled);
        assert!(settings.validate().is_ok());
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
    fn smtp_validation_requires_the_useful_fields_only_when_enabled() {
        assert!(CacheAlertSettings::default().validate().is_ok());

        let missing_host = CacheAlertSettings {
            smtp: CacheAlertSmtpSettings {
                host: String::new(),
                ..smtp(true)
            },
            ..CacheAlertSettings::default()
        };
        assert!(missing_host.validate().is_err());

        let missing_recipients = CacheAlertSettings {
            smtp: CacheAlertSmtpSettings {
                to: Vec::new(),
                ..smtp(true)
            },
            ..CacheAlertSettings::default()
        };
        assert!(missing_recipients.validate().is_err());

        let invalid_address = CacheAlertSettings {
            smtp: CacheAlertSmtpSettings {
                to: vec!["not-an-address".to_string()],
                ..smtp(true)
            },
            ..CacheAlertSettings::default()
        };
        assert!(invalid_address.validate().is_err());

        let valid = CacheAlertSettings {
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        };
        assert!(valid.validate().is_ok());

        let disabled_draft = CacheAlertSettings {
            smtp: CacheAlertSmtpSettings {
                enabled: false,
                host: "smtp.example.com".to_string(),
                ..CacheAlertSmtpSettings::default()
            },
            ..CacheAlertSettings::default()
        };
        assert!(disabled_draft.validate().is_ok());
    }

    #[test]
    fn smtp_validation_keeps_credentials_paired_and_never_echoes_them() {
        let half_credentials = CacheAlertSettings {
            smtp: CacheAlertSmtpSettings {
                username: "alert-user".to_string(),
                password: String::new(),
                ..smtp(true)
            },
            ..CacheAlertSettings::default()
        };
        let error = half_credentials
            .validate()
            .expect_err("unpaired SMTP credentials must fail");
        let text = error.to_string();
        assert!(!text.contains("alert-user"), "{text}");
        assert!(!text.contains("SEC-smtp-password"), "{text}");
        assert!(text.contains("set together"), "{text}");

        let invalid_host = CacheAlertSettings {
            smtp: CacheAlertSmtpSettings {
                host: "smtp://smtp.example.com".to_string(),
                ..smtp(true)
            },
            ..CacheAlertSettings::default()
        };
        let text = invalid_host
            .validate()
            .expect_err("a scheme in the host must fail")
            .to_string();
        assert!(!text.contains("smtp.example.com"), "{text}");
    }

    #[test]
    fn smtp_debug_output_redacts_identity_and_credentials() {
        let settings = CacheAlertSettings {
            dingtalk_secret: "SEC-dingtalk".to_string(),
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        };

        let text = format!("{settings:?}");
        assert!(!text.contains("SEC-smtp-password"), "{text}");
        assert!(!text.contains("SEC-dingtalk"), "{text}");
        assert!(!text.contains("alert-user"), "{text}");
        assert!(!text.contains("alerts@example.com"), "{text}");
        assert!(!text.contains("ops@example.com"), "{text}");
    }

    #[test]
    fn cache_alert_redacted_view_hides_the_stored_secrets() {
        let settings = CacheAlertSettings {
            dingtalk_secret: "secret".to_string(),
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        };

        assert_eq!(settings.redacted().dingtalk_secret, "");
        assert_eq!(settings.redacted().smtp.password, "");
    }

    #[test]
    fn cache_alert_response_reports_presence_and_never_echoes_the_secrets() {
        let absent = CacheAlertSettings::default().redacted_response();
        assert!(!absent.has_dingtalk_secret);
        assert_eq!(absent.dingtalk_secret, "");
        assert!(
            !absent
                .smtp
                .expect("SMTP view is always present")
                .has_smtp_password
        );

        let present = CacheAlertSettings {
            dingtalk_secret: "SEC-test-secret".to_string(),
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        }
        .redacted_response();
        assert!(present.has_dingtalk_secret);
        assert_eq!(present.dingtalk_secret, "");
        let smtp_view = present.smtp.expect("SMTP view is always present");
        assert!(smtp_view.has_smtp_password);
        assert_eq!(smtp_view.password, "");

        let blank = CacheAlertSettings {
            dingtalk_secret: "   ".to_string(),
            smtp: CacheAlertSmtpSettings {
                password: "   ".to_string(),
                ..smtp(true)
            },
            ..CacheAlertSettings::default()
        }
        .redacted_response();
        assert!(!blank.has_dingtalk_secret);
        assert!(
            !blank
                .smtp
                .expect("SMTP view is always present")
                .has_smtp_password
        );
    }

    #[test]
    fn cache_alert_request_never_carries_the_derived_presence_flag() {
        let request = serde_json::to_value(CacheAlertSettings {
            dingtalk_secret: "SEC-test-secret".to_string(),
            ..CacheAlertSettings::default()
        })
        .expect("serialize request");

        assert!(request.get("has_dingtalk_secret").is_none());
        assert_eq!(request["dingtalk_secret"], "SEC-test-secret");

        // An unexpected presence flag in a payload is not persisted either.
        let parsed: CacheAlertSettings =
            serde_json::from_value(json!({ "has_dingtalk_secret": true }))
                .expect("partial payload");
        assert!(parsed.dingtalk_secret.is_empty());
    }

    #[test]
    fn omitted_smtp_request_keeps_the_stored_configuration() {
        let stored = CacheAlertSettings {
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        };
        let request: CacheAlertSettingsRequest =
            serde_json::from_value(json!({ "enabled": true })).expect("legacy request");

        let merged = request.into_settings(&stored);
        assert!(merged.enabled);
        assert!(merged.smtp.enabled);
        assert_eq!(merged.smtp.host, "smtp.example.com");
        assert_eq!(merged.smtp.password, "SEC-smtp-password");
    }

    #[test]
    fn explicit_smtp_request_replaces_or_disables_the_stored_configuration() {
        let stored = CacheAlertSettings {
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        };
        let request: CacheAlertSettingsRequest = serde_json::from_value(json!({
            "enabled": true,
            "smtp": {
                "enabled": false,
                "host": "smtp.other.example",
                "port": 465,
                "tls": "implicit_tls",
                "from": "alerts@other.example",
                "to": ["oncall@other.example"],
                "username": "",
                "password": ""
            }
        }))
        .expect("explicit request");

        let merged = request.into_settings(&stored);
        assert!(merged.enabled);
        assert!(!merged.smtp.enabled);
        assert_eq!(merged.smtp.host, "smtp.other.example");
        assert!(merged.smtp.username.is_empty());
        assert!(merged.smtp.password.is_empty());
        assert!(merged.validate().is_ok());
    }

    #[test]
    fn minimal_explicit_disable_closes_the_stored_smtp_channel() {
        let stored = CacheAlertSettings {
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        };
        let request: CacheAlertSettingsRequest =
            serde_json::from_value(json!({ "smtp": { "enabled": false } }))
                .expect("minimal disable request");

        let merged = request.into_settings(&stored);
        assert!(!merged.smtp.enabled);
        assert!(merged.smtp.host.is_empty());
        assert!(merged.smtp.to.is_empty());
        assert!(merged.smtp.username.is_empty());
        assert!(merged.smtp.password.is_empty());
        assert!(merged.validate().is_ok());
    }

    #[test]
    fn blank_smtp_password_keeps_the_stored_one() {
        let stored = CacheAlertSettings {
            smtp: smtp(true),
            ..CacheAlertSettings::default()
        };
        let request: CacheAlertSettingsRequest = serde_json::from_value(json!({
            "smtp": {
                "enabled": true,
                "host": "smtp.example.com",
                "port": 587,
                "tls": "start_tls",
                "from": "alerts@example.com",
                "to": ["ops@example.com"],
                "username": "alert-user",
                "password": ""
            }
        }))
        .expect("request with blank password");

        let merged = request.into_settings(&stored);
        assert_eq!(merged.smtp.password, "SEC-smtp-password");
        assert!(merged.validate().is_ok());
    }
}
