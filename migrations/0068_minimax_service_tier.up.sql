ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS service_tier TEXT NOT NULL DEFAULT 'standard';

UPDATE provider_endpoints
SET service_tier = 'standard'
WHERE service_tier IS NULL OR service_tier NOT IN ('standard', 'priority');

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_service_tier;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_service_tier
CHECK (service_tier IN ('standard', 'priority'));
