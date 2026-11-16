-- Restore the schema shape of the removed approval gate. The terminalized
-- request rows and the deleted approval/settings values are not recovered.

CREATE TABLE IF NOT EXISTS approval_requests (
    approval_id UUID PRIMARY KEY,
    request_id UUID NOT NULL UNIQUE,
    user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    client_key_label TEXT,
    path TEXT NOT NULL,
    model TEXT,
    review_decision TEXT NOT NULL,
    approval_status TEXT NOT NULL,
    review_reason TEXT NOT NULL DEFAULT '',
    review_categories TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
    request_preview TEXT NOT NULL,
    request_payload_json JSONB,
    request_deadline_unix_ms BIGINT NOT NULL DEFAULT 0,
    wait_deadline_unix_ms BIGINT NOT NULL DEFAULT 0,
    decided_by_user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    decided_at TIMESTAMPTZ,
    webhook_last_error TEXT,
    webhook_last_attempted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_approval_requests_review_decision
        CHECK (review_decision IN ('allow', 'flag', 'error', 'timeout')),
    CONSTRAINT ck_approval_requests_approval_status
        CHECK (approval_status IN ('pending', 'approved', 'rejected', 'expired', 'aborted'))
);

CREATE INDEX IF NOT EXISTS idx_approval_requests_status_created_at
ON approval_requests(approval_status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_approval_requests_resolved_created_at
ON approval_requests(created_at ASC, approval_id ASC)
WHERE approval_status <> 'pending';

ALTER TABLE request_records
    DROP CONSTRAINT IF EXISTS ck_request_records_request_state;

ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_request_state CHECK (
        request_state IN (
            'received', 'awaiting_approval', 'upstream_processing', 'completed', 'failed', 'aborted'
        )
    );

ALTER TABLE request_records
    DROP CONSTRAINT IF EXISTS ck_request_records_abort_from_state;

ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_abort_from_state CHECK (
        abort_from_state IS NULL
        OR abort_from_state IN ('received', 'awaiting_approval', 'upstream_processing')
    );
