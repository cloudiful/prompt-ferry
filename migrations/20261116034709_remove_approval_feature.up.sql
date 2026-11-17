-- The human approval and dedicated LLM review chain is removed. Requests that
-- were still parked on the removed gate would otherwise keep a lifecycle state
-- the tightened constraints no longer accept, so they are terminalized first.

DELETE FROM request_record_leases lease
USING request_records rr
WHERE lease.request_id = rr.request_id
  AND rr.event_kind = 'request'
  AND rr.request_state = 'awaiting_approval';

UPDATE request_records
SET request_state = 'aborted',
    ok = FALSE,
    error_code = COALESCE(error_code, 'approval_feature_removed'),
    error_message = COALESCE(
        error_message,
        'approval gate removed before the request was forwarded upstream'
    ),
    failure_family = COALESCE(failure_family, 'policy'),
    abort_from_state = NULL,
    abort_response_started = NULL,
    updated_at = NOW()
WHERE event_kind = 'request'
  AND request_state = 'awaiting_approval';

ALTER TABLE request_records
    DROP CONSTRAINT IF EXISTS ck_request_records_request_state;

ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_request_state CHECK (
        request_state IN ('received', 'upstream_processing', 'completed', 'failed', 'aborted')
    );

ALTER TABLE request_records
    DROP CONSTRAINT IF EXISTS ck_request_records_abort_from_state;

ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_abort_from_state CHECK (
        abort_from_state IS NULL OR abort_from_state IN ('received', 'upstream_processing')
    );

DROP TABLE IF EXISTS approval_requests;

DELETE FROM worker_settings
WHERE setting_key = 'llm_review_settings';

UPDATE worker_settings
SET setting_value = setting_value - 'approval_retention_days',
    updated_at = NOW()
WHERE setting_key = 'usage_retention'
  AND jsonb_typeof(setting_value) = 'object'
  AND setting_value ? 'approval_retention_days';
