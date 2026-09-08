-- Reverse 0074: fold any OpenRouter rows back to generic (which shares
-- the NULL-region shape) so the narrower CHECKs below apply cleanly.
UPDATE provider_endpoints
SET provider = 'generic', provider_region = NULL
WHERE provider = 'openrouter';

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider_region;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider
CHECK (provider IN ('generic', 'minimax', 'command_code', 'opencode_go'));

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider_region
CHECK (
    (provider = 'generic' AND provider_region IS NULL)
    OR (provider = 'minimax' AND provider_region IN ('cn', 'global'))
    OR (provider = 'command_code' AND provider_region IS NULL)
    OR (provider = 'opencode_go' AND provider_region IS NULL)
);
