use super::{ObservationQueryError, parse_observation_query, parse_trend_kinds};
use crate::worker_admin_types::{CanonicalQuotaWindowKind, QuotaKeyObservationHistoryQuery};
use uuid::Uuid;

#[test]
fn observation_query_defaults_and_enforces_bounded_pages() {
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery::default()).unwrap(),
        (50, None, None)
    );
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery {
            key_id: None,
            key_ids: None,
            limit: Some(1),
            before_id: None,
        })
        .unwrap(),
        (1, None, None)
    );
    for limit in [0, 201] {
        assert_eq!(
            parse_observation_query(&QuotaKeyObservationHistoryQuery {
                key_id: None,
                key_ids: None,
                limit: Some(limit),
                before_id: None,
            }),
            Err(ObservationQueryError::InvalidLimit)
        );
    }
}

#[test]
fn observation_cursor_and_key_id_reject_bad_shapes() {
    let key = Uuid::new_v4();
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery {
            key_id: Some(key.to_string()),
            key_ids: None,
            limit: None,
            before_id: Some(i64::MAX.to_string()),
        })
        .unwrap(),
        (50, Some(i64::MAX), Some(vec![key]))
    );
    for before_id in ["0", "-1", "1.5", " 1", "9223372036854775808"] {
        assert_eq!(
            parse_observation_query(&QuotaKeyObservationHistoryQuery {
                key_id: None,
                key_ids: None,
                limit: None,
                before_id: Some(before_id.to_string()),
            }),
            Err(ObservationQueryError::InvalidCursor),
            "cursor={before_id}"
        );
    }
    for key_id in ["", "zzz", "8c6e4e55-967c-4267-a45a-24647d2afad"] {
        assert_eq!(
            parse_observation_query(&QuotaKeyObservationHistoryQuery {
                key_id: Some(key_id.to_string()),
                key_ids: None,
                limit: None,
                before_id: None,
            }),
            Err(ObservationQueryError::InvalidKey),
            "key_id={key_id}"
        );
    }
}

#[test]
fn observation_key_ids_filter_is_deduped_bounded_and_exclusive() {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery {
            key_id: None,
            key_ids: Some(format!("{first},{first},{second}")),
            limit: None,
            before_id: None,
        })
        .unwrap(),
        (50, None, Some(vec![first, second]))
    );
    assert_eq!(
        parse_observation_query(&QuotaKeyObservationHistoryQuery {
            key_id: Some(first.to_string()),
            key_ids: Some(second.to_string()),
            limit: None,
            before_id: None,
        }),
        Err(ObservationQueryError::ConflictingKeyFilters)
    );
    let over_cap = (0..25)
        .map(|_| Uuid::new_v4().to_string())
        .collect::<Vec<_>>()
        .join(",");
    for key_ids in [
        String::new(),
        format!("{first},"),
        format!("{first},,{second}"),
        format!("{first},not-a-uuid"),
        over_cap,
    ] {
        assert_eq!(
            parse_observation_query(&QuotaKeyObservationHistoryQuery {
                key_id: None,
                key_ids: Some(key_ids.clone()),
                limit: None,
                before_id: None,
            }),
            Err(ObservationQueryError::InvalidKeyIds),
            "key_ids={key_ids}"
        );
    }
}

#[test]
fn trend_kinds_accept_canonical_values_and_reject_unknown_or_blank() {
    assert_eq!(parse_trend_kinds(None).unwrap(), None);
    let parsed = parse_trend_kinds(Some("weekly, rolling,weekly"))
        .unwrap()
        .expect("known kinds parse");
    assert_eq!(
        parsed.into_iter().collect::<Vec<_>>(),
        vec![
            CanonicalQuotaWindowKind::Weekly,
            CanonicalQuotaWindowKind::Rolling
        ]
    );
    for kinds in ["", "five_hour,", "weekly,,balance", "fortnightly"] {
        assert_eq!(
            parse_trend_kinds(Some(kinds)),
            Err(ObservationQueryError::InvalidKinds),
            "kinds={kinds}"
        );
    }
}
