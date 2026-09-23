-- Reverse of the #584 constraint pin: restore the pre-fix native_api kind set
-- and the 0079 region shape. Rows the narrower CHECKs cannot hold are folded
-- first, mirroring the 0052 down migration.
UPDATE provider_endpoints
SET native_api = 'responses', native_api_source = 'manual'
WHERE native_api = 'realtime';

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api
CHECK (native_api IN ('auto', 'responses', 'chat', 'anthropic_messages'));

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api_source;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api_source
CHECK (native_api_source IN ('auto', 'detected', 'manual'));

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider_region;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider_region
CHECK (
    (provider = 'generic' AND provider_region IS NULL)
    OR (provider = 'minimax' AND provider_region IN ('cn', 'global'))
    OR (provider = 'command_code' AND provider_region IS NULL)
    OR (provider = 'opencode_go' AND provider_region IS NULL)
    OR (provider = 'openrouter' AND provider_region IS NULL)
    OR (provider = 'glm' AND provider_region IS NULL)
    OR (provider = 'deepseek' AND provider_region IS NULL)
);
