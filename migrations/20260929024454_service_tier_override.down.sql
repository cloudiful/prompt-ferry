ALTER TABLE model_route_targets
DROP COLUMN IF EXISTS service_tier;

-- Restore the legacy fixed enum: unset and any free-form value that the old
-- CHECK did not allow fold back to the pre-#637 `standard` default.
UPDATE provider_endpoints
SET service_tier = 'standard'
WHERE service_tier IS NULL OR service_tier NOT IN ('standard', 'priority');

ALTER TABLE provider_endpoints
ALTER COLUMN service_tier SET DEFAULT 'standard';

ALTER TABLE provider_endpoints
ALTER COLUMN service_tier SET NOT NULL;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_service_tier;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_service_tier
CHECK (service_tier IN ('standard', 'priority'));
