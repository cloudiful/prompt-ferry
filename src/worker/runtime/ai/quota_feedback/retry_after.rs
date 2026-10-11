use http::{HeaderMap, header::RETRY_AFTER};
use std::time::Duration;

const MAX_RETRY_AFTER: Duration = Duration::from_secs(3600);

pub(super) fn parse(headers: &HeaderMap, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    let values = headers.get_all(RETRY_AFTER);
    let mut values = values.iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let value = value.to_str().ok()?.trim();
    if value.is_empty() {
        return None;
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(Duration::from_secs(
            value
                .parse::<u64>()
                .unwrap_or(u64::MAX)
                .min(MAX_RETRY_AFTER.as_secs()),
        ));
    }
    if !value.ends_with(" GMT") {
        return None;
    }
    let deadline = chrono::DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&chrono::Utc);
    let remaining = deadline.signed_duration_since(now);
    if remaining <= chrono::Duration::zero() {
        return Some(Duration::ZERO);
    }
    Some(
        remaining
            .to_std()
            .unwrap_or(MAX_RETRY_AFTER)
            .min(MAX_RETRY_AFTER),
    )
}
