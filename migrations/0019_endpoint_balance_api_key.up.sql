ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS balance_api_key TEXT;
