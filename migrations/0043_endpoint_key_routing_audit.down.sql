ALTER TABLE conversation_endpoint_overrides
DROP COLUMN IF EXISTS endpoint_key_id;

ALTER TABLE request_records
DROP COLUMN IF EXISTS endpoint_key_id,
DROP COLUMN IF EXISTS endpoint_key_label;
