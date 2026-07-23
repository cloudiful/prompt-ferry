ALTER TABLE model_endpoint_rules
ADD COLUMN IF NOT EXISTS stream_output_coalescing_override_json JSONB NOT NULL DEFAULT 'null'::JSONB;
