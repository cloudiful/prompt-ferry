DROP INDEX IF EXISTS idx_request_records_raw_retention_created_at;

CREATE INDEX IF NOT EXISTS idx_request_records_raw_retention_created_at
ON request_records(created_at)
WHERE request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL;
