ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider_region;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider;

ALTER TABLE provider_endpoints
DROP COLUMN IF EXISTS provider_region;

ALTER TABLE provider_endpoints
DROP COLUMN IF EXISTS provider;
