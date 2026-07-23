DROP TABLE IF EXISTS endpoint_api_keys;

ALTER TABLE provider_endpoints
DROP COLUMN IF EXISTS key_lb_enabled;
