ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_service_tier;

ALTER TABLE provider_endpoints
DROP COLUMN IF EXISTS service_tier;
