-- Issue #637: replace the MiniMax-only fixed service-tier enum with a generic
-- free-form override. `provider_endpoints.service_tier` is now an optional
-- trimmed string; NULL/blank means inherit (leave the caller/provider default
-- untouched). Model-route targets gain their own optional override that wins
-- over the endpoint value during route resolution.
ALTER TABLE provider_endpoints
ALTER COLUMN service_tier DROP NOT NULL;

ALTER TABLE provider_endpoints
ALTER COLUMN service_tier DROP DEFAULT;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_service_tier;

-- Trim stored values and treat blank as unset.
UPDATE provider_endpoints
SET service_tier = NULLIF(btrim(service_tier), '')
WHERE service_tier IS NOT NULL;

-- The legacy `standard` default equals the provider default, so it is
-- semantically an inherit and migrates to NULL. Explicit non-default tiers
-- (e.g. `priority`) survive.
UPDATE provider_endpoints
SET service_tier = NULL
WHERE service_tier = 'standard';

ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS service_tier TEXT;
