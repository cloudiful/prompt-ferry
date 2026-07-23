-- Destructive storage migration: existing raw payload columns are intentionally
-- discarded instead of copied into the new retention-managed table.
DROP INDEX IF EXISTS idx_request_records_raw_retention_created_at;

CREATE TABLE IF NOT EXISTS request_record_raw_payloads (
    event_id BIGINT NOT NULL REFERENCES request_records(event_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    request_raw_json JSONB,
    response_raw_body TEXT,
    CONSTRAINT ck_request_record_raw_payloads_has_payload CHECK (
        request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL
    ),
    PRIMARY KEY (created_at, event_id)
) PARTITION BY RANGE (created_at);

CREATE TABLE IF NOT EXISTS request_record_raw_payloads_default
PARTITION OF request_record_raw_payloads DEFAULT;

CREATE TABLE IF NOT EXISTS request_record_raw_payloads_overflow (
    event_id BIGINT NOT NULL REFERENCES request_records(event_id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    request_raw_json JSONB,
    response_raw_body TEXT,
    CONSTRAINT ck_request_record_raw_payloads_overflow_has_payload CHECK (
        request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL
    ),
    PRIMARY KEY (created_at, event_id)
);

CREATE INDEX IF NOT EXISTS idx_request_record_raw_payloads_event_id
ON request_record_raw_payloads(event_id);

CREATE INDEX IF NOT EXISTS idx_request_record_raw_payloads_created_at_event_id
ON request_record_raw_payloads(created_at, event_id);

ALTER TABLE request_records
DROP COLUMN IF EXISTS request_raw_json,
DROP COLUMN IF EXISTS response_raw_body;
