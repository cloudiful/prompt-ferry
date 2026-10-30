mod cache_alert_email;
mod cache_alert_sender;

pub use cache_alert_email::send_cache_alert_email;
pub use cache_alert_sender::send_cache_alert;

/// Stable channel names shared by the cooldown rows and the alert monitor.
pub const CACHE_ALERT_CHANNEL_DINGTALK: &str = "dingtalk";
pub const CACHE_ALERT_CHANNEL_SMTP: &str = "smtp";
