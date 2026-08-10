use chrono::{DateTime, Datelike, Duration as ChronoDuration, TimeZone, Utc};

use crate::db::types::{McpQuotaGroup, QuotaPeriod, QuotaPeriodKind};

pub fn current_day_period(now: DateTime<Utc>) -> QuotaPeriod {
    let start = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .expect("valid UTC day start");
    QuotaPeriod {
        kind: QuotaPeriodKind::Day,
        start,
        end: start + ChronoDuration::days(1),
    }
}

/// The current billing window for a group's monthly quota. A configured
/// `billing_period_start`/`billing_period_end` pair is treated as a repeating
/// window anchored at `billing_period_start`; without one the window is the
/// UTC calendar month.
pub fn current_month_period(group: &McpQuotaGroup, now: DateTime<Utc>) -> QuotaPeriod {
    if let (Some(anchor), Some(end)) = (group.billing_period_start, group.billing_period_end)
        && end > anchor
    {
        let window_seconds = (end - anchor).num_seconds();
        if window_seconds > 0 {
            let elapsed = (now - anchor).num_seconds();
            let cycles = elapsed.div_euclid(window_seconds);
            let period_start = anchor + ChronoDuration::seconds(cycles * window_seconds);
            return QuotaPeriod {
                kind: QuotaPeriodKind::Month,
                start: period_start,
                end: period_start + ChronoDuration::seconds(window_seconds),
            };
        }
    }
    let start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid UTC month start");
    let (next_year, next_month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    let end = Utc
        .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
        .single()
        .expect("valid UTC next month start");
    QuotaPeriod {
        kind: QuotaPeriodKind::Month,
        start,
        end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn group_with_billing(
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> McpQuotaGroup {
        McpQuotaGroup {
            group_id: uuid::Uuid::nil(),
            name: "test".to_string(),
            scope: "admin".to_string(),
            owner_user_id: None,
            provider_kind: None,
            unit: "requests".to_string(),
            daily_limit: None,
            monthly_limit: Some(100.0),
            default_cost: 1.0,
            strict_mode: false,
            billing_period_start: start,
            billing_period_end: end,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.from_utc_datetime(
            &NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        )
    }

    #[test]
    fn day_period_is_utc_calendar_day() {
        let period = current_day_period(utc(2026, 8, 9).with_timezone(&Utc));
        assert_eq!(period.start, utc(2026, 8, 9));
        assert_eq!(period.end, utc(2026, 8, 10));
    }

    #[test]
    fn month_period_defaults_to_utc_calendar_month() {
        let period = current_month_period(&group_with_billing(None, None), utc(2026, 8, 15));
        assert_eq!(period.start, utc(2026, 8, 1));
        assert_eq!(period.end, utc(2026, 9, 1));
    }

    #[test]
    fn month_period_follows_repeating_billing_window() {
        let group = group_with_billing(Some(utc(2026, 7, 15)), Some(utc(2026, 8, 15)));
        let period = current_month_period(&group, utc(2026, 9, 10));
        assert_eq!(period.start, utc(2026, 8, 15));
        assert_eq!(period.end, utc(2026, 9, 15));
    }

    #[test]
    fn month_period_anchors_before_window_start() {
        let group = group_with_billing(Some(utc(2026, 8, 15)), Some(utc(2026, 9, 15)));
        let period = current_month_period(&group, utc(2026, 8, 1));
        assert_eq!(period.start, utc(2026, 7, 15));
        assert_eq!(period.end, utc(2026, 8, 15));
    }
}
