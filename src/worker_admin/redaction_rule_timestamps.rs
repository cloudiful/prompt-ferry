use chrono::{DateTime, Utc};

use crate::redact::RedactionCustomStringRule;

/// Stamp per-rule `created_at` / `updated_at` for one redaction config save.
///
/// Identity is positional: the rule at index `i` continues the rule previously
/// stored at index `i`. Content changes bump `updated_at`; a rule beyond the
/// previous length is new; a rule that moved (client echoed its `created_at`)
/// keeps that `created_at`. Rows stored before this feature existed fall back
/// to the config blob's own `updated_at`. Every value is clamped to `now`.
pub fn stamp_custom_string_timestamps(
    previous: &[RedactionCustomStringRule],
    previous_blob_updated_at: Option<DateTime<Utc>>,
    next: &mut [RedactionCustomStringRule],
    now: DateTime<Utc>,
) {
    for (index, rule) in next.iter_mut().enumerate() {
        let previous_rule = previous.get(index);
        let unchanged = previous_rule.is_some_and(|previous| previous.same_content(rule));
        let legacy_created_at = previous_rule.and_then(|_| previous_blob_updated_at);

        let created_at = rule
            .created_at
            .or_else(|| previous_rule.and_then(|previous| previous.created_at))
            .or(legacy_created_at)
            .unwrap_or(now)
            .min(now);

        let updated_at = if unchanged {
            rule.updated_at
                .or_else(|| previous_rule.and_then(|previous| previous.updated_at))
                .or(legacy_created_at)
                .unwrap_or(now)
                .min(now)
        } else {
            now
        };

        rule.created_at = Some(created_at);
        rule.updated_at = Some(updated_at.max(created_at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redactor::{CustomStringMatch, CustomStringScope};

    fn rule(pattern: &str) -> RedactionCustomStringRule {
        RedactionCustomStringRule {
            pattern: pattern.to_string(),
            match_type: CustomStringMatch::Exact,
            scope: CustomStringScope::Text,
            ..Default::default()
        }
    }

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("rfc3339")
            .with_timezone(&Utc)
    }

    #[test]
    fn new_rules_are_stamped_with_now() {
        let now = at("2026-09-19T10:00:00Z");
        let mut next = vec![rule("acme")];

        stamp_custom_string_timestamps(&[], None, &mut next, now);

        assert_eq!(next[0].created_at, Some(now));
        assert_eq!(next[0].updated_at, Some(now));
    }

    #[test]
    fn unchanged_rules_keep_their_timestamps() {
        let created = at("2026-01-01T00:00:00Z");
        let updated = at("2026-02-01T00:00:00Z");
        let mut previous = vec![rule("acme")];
        previous[0].created_at = Some(created);
        previous[0].updated_at = Some(updated);
        let mut next = previous.clone();

        stamp_custom_string_timestamps(&previous, None, &mut next, at("2026-09-19T10:00:00Z"));

        assert_eq!(next[0].created_at, Some(created));
        assert_eq!(next[0].updated_at, Some(updated));
    }

    #[test]
    fn edited_rules_keep_created_at_and_bump_updated_at() {
        let created = at("2026-01-01T00:00:00Z");
        let mut previous = vec![rule("acme")];
        previous[0].created_at = Some(created);
        previous[0].updated_at = Some(created);
        let now = at("2026-09-19T10:00:00Z");
        let mut next = vec![rule("acme-updated")];
        next[0].created_at = Some(created);
        next[0].updated_at = Some(created);

        stamp_custom_string_timestamps(&previous, None, &mut next, now);

        assert_eq!(next[0].created_at, Some(created));
        assert_eq!(next[0].updated_at, Some(now));
    }

    #[test]
    fn legacy_rows_are_backfilled_from_the_blob_timestamp() {
        let blob = at("2026-07-27T02:13:27Z");
        let previous = vec![rule("acme")];
        let mut next = vec![rule("acme")];

        stamp_custom_string_timestamps(
            &previous,
            Some(blob),
            &mut next,
            at("2026-09-19T10:00:00Z"),
        );

        assert_eq!(next[0].created_at, Some(blob));
        assert_eq!(next[0].updated_at, Some(blob));
    }

    #[test]
    fn moved_rules_keep_the_client_created_at() {
        let created = at("2026-03-03T00:00:00Z");
        let now = at("2026-09-19T10:00:00Z");
        let previous = vec![rule("first"), rule("second")];
        let mut next = vec![rule("second")];
        next[0].created_at = Some(created);

        stamp_custom_string_timestamps(&previous, None, &mut next, now);

        assert_eq!(next[0].created_at, Some(created));
        assert_eq!(next[0].updated_at, Some(now));
    }

    #[test]
    fn client_timestamps_are_clamped_to_now() {
        let now = at("2026-09-19T10:00:00Z");
        let future = at("2030-01-01T00:00:00Z");
        let mut next = vec![rule("acme")];
        next[0].created_at = Some(future);
        next[0].updated_at = Some(future);

        stamp_custom_string_timestamps(&[], None, &mut next, now);

        assert_eq!(next[0].created_at, Some(now));
        assert_eq!(next[0].updated_at, Some(now));
    }
}
