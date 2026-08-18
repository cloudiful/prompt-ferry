ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS provider TEXT NOT NULL DEFAULT 'generic';

ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS provider_region TEXT;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider
CHECK (provider IN ('generic', 'minimax'));

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider_region;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider_region
CHECK (
    (provider = 'generic' AND provider_region IS NULL)
    OR (provider = 'minimax' AND provider_region IN ('cn', 'global'))
);
