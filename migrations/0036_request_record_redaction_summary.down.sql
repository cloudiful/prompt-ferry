DROP INDEX IF EXISTS idx_request_records_redaction_applied_created_at;

ALTER TABLE request_records
DROP COLUMN IF EXISTS redaction_fields_json;

ALTER TABLE request_records
DROP COLUMN IF EXISTS redaction_types_json;

ALTER TABLE request_records
DROP COLUMN IF EXISTS redaction_replacements_count;

ALTER TABLE request_records
DROP COLUMN IF EXISTS redaction_findings_count;

ALTER TABLE request_records
DROP COLUMN IF EXISTS redaction_applied;
