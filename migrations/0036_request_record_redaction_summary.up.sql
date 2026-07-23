ALTER TABLE request_records
ADD COLUMN redaction_applied BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE request_records
ADD COLUMN redaction_findings_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE request_records
ADD COLUMN redaction_replacements_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE request_records
ADD COLUMN redaction_types_json JSONB;

ALTER TABLE request_records
ADD COLUMN redaction_fields_json JSONB;

CREATE INDEX IF NOT EXISTS idx_request_records_redaction_applied_created_at
ON request_records(redaction_applied, created_at DESC);
