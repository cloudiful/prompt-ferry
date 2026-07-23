-- Rollback is destructive: raw payloads stored in the partitioned table are lost.
DROP TABLE IF EXISTS request_record_raw_payloads CASCADE;
DROP TABLE IF EXISTS request_record_raw_payloads_overflow;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS request_raw_json JSONB,
ADD COLUMN IF NOT EXISTS response_raw_body TEXT;

CREATE INDEX IF NOT EXISTS idx_request_records_raw_retention_created_at
ON request_records(created_at, event_id)
WHERE request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL;
