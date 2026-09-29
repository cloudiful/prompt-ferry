-- P6 (issue #570): MiniMax endpoints must carry a provider region (`cn`
-- or `global`). PostgreSQL CHECKs only reject FALSE, so the legacy
-- `provider_region IN ('cn', 'global')` silently accepted NULL. The tightened
-- branch below adds an explicit `IS NOT NULL`; every other provider branch
-- keeps its exact legacy shape.
--
-- Pre-existing NULL-region MiniMax rows would violate the tightened CHECK,
-- and business data must never be dropped or rewritten by a migration, so
-- abort loudly with the offending endpoint names instead of touching rows.
DO $$
DECLARE
    offenders TEXT;
BEGIN
    SELECT string_agg(name, ', ' ORDER BY name)
    INTO offenders
    FROM provider_endpoints
    WHERE provider = 'minimax'
      AND COALESCE(provider_region, '') NOT IN ('cn', 'global');
    IF offenders IS NOT NULL THEN
        RAISE EXCEPTION 'cannot tighten MiniMax provider_region: endpoints without a valid region: %', offenders;
    END IF;
END $$;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider_region;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider_region
CHECK (
    (provider = 'generic' AND provider_region IS NULL)
    OR (provider = 'minimax' AND provider_region IS NOT NULL AND provider_region IN ('cn', 'global'))
    OR (provider = 'command_code' AND provider_region IS NULL)
    OR (provider = 'opencode_go' AND provider_region IS NULL)
    OR (provider = 'openrouter' AND provider_region IS NULL)
    OR (provider = 'glm' AND provider_region IS NULL)
    OR (provider = 'deepseek' AND provider_region IS NULL)
    OR (provider = 'openai' AND provider_region IS NULL)
);
