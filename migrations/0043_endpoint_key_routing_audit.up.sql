ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS endpoint_key_id UUID,
ADD COLUMN IF NOT EXISTS endpoint_key_label TEXT;

ALTER TABLE conversation_endpoint_overrides
ADD COLUMN IF NOT EXISTS endpoint_key_id UUID REFERENCES endpoint_api_keys(key_id) ON DELETE SET NULL;
