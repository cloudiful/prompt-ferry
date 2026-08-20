//! Shared fixture builders used by the backfill unit tests.

use chrono::Utc;
use uuid::Uuid;

use crate::db::billing::backfill::BackfillCandidate;

pub fn candidate(
    event_id: i64,
    existing_input_tokens: Option<i64>,
    existing_cache_read_tokens: Option<i64>,
    existing_cache_write_tokens: Option<i64>,
    response_in_postgres: bool,
    raw_object_only: bool,
    response_capture_truncated: bool,
) -> BackfillCandidate {
    BackfillCandidate {
        event_id,
        request_id: Uuid::nil(),
        created_at: Utc::now(),
        requested_model: Some("public-model".to_string()),
        upstream_model: Some("upstream-model".to_string()),
        model: Some("public-model".to_string()),
        existing_input_tokens,
        existing_output_tokens: Some(42),
        existing_total_tokens: Some(83018),
        existing_cached_tokens: existing_cache_read_tokens,
        existing_cache_read_tokens,
        existing_cache_write_tokens,
        response_in_postgres,
        request_in_postgres: false,
        raw_object_only,
        response_capture_truncated,
    }
}

pub fn candidate_with_models(
    event_id: i64,
    requested_model: Option<&str>,
    model: Option<&str>,
) -> BackfillCandidate {
    BackfillCandidate {
        event_id,
        request_id: Uuid::nil(),
        created_at: Utc::now(),
        requested_model: requested_model.map(str::to_string),
        upstream_model: Some("upstream-model".to_string()),
        model: model.map(str::to_string),
        existing_input_tokens: Some(100),
        existing_output_tokens: Some(20),
        existing_total_tokens: Some(120),
        existing_cached_tokens: None,
        existing_cache_read_tokens: None,
        existing_cache_write_tokens: None,
        response_in_postgres: true,
        request_in_postgres: false,
        raw_object_only: false,
        response_capture_truncated: false,
    }
}

pub fn equal_some_candidate(event_id: i64) -> BackfillCandidate {
    BackfillCandidate {
        event_id,
        request_id: Uuid::nil(),
        created_at: Utc::now(),
        requested_model: Some("public-model".to_string()),
        upstream_model: Some("upstream-model".to_string()),
        model: Some("public-model".to_string()),
        existing_input_tokens: Some(7),
        existing_output_tokens: Some(7),
        existing_total_tokens: Some(7),
        existing_cached_tokens: Some(7),
        existing_cache_read_tokens: Some(7),
        existing_cache_write_tokens: Some(7),
        response_in_postgres: true,
        request_in_postgres: false,
        raw_object_only: false,
        response_capture_truncated: false,
    }
}

pub fn none_candidate(event_id: i64) -> BackfillCandidate {
    BackfillCandidate {
        event_id,
        request_id: Uuid::nil(),
        created_at: Utc::now(),
        requested_model: Some("public-model".to_string()),
        upstream_model: Some("upstream-model".to_string()),
        model: Some("public-model".to_string()),
        existing_input_tokens: None,
        existing_output_tokens: None,
        existing_total_tokens: None,
        existing_cached_tokens: None,
        existing_cache_read_tokens: None,
        existing_cache_write_tokens: None,
        response_in_postgres: true,
        request_in_postgres: false,
        raw_object_only: false,
        response_capture_truncated: false,
    }
}

pub fn stored_some_candidate(event_id: i64) -> BackfillCandidate {
    BackfillCandidate {
        event_id,
        request_id: Uuid::nil(),
        created_at: Utc::now(),
        requested_model: Some("public-model".to_string()),
        upstream_model: Some("upstream-model".to_string()),
        model: Some("public-model".to_string()),
        existing_input_tokens: Some(120),
        existing_output_tokens: Some(30),
        existing_total_tokens: Some(150),
        existing_cached_tokens: Some(30),
        existing_cache_read_tokens: Some(30),
        existing_cache_write_tokens: Some(7),
        response_in_postgres: true,
        request_in_postgres: false,
        raw_object_only: false,
        response_capture_truncated: false,
    }
}
