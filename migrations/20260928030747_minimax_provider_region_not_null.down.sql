-- Reverse the MiniMax region tightening: restore the exact legacy CHECK
-- text from 20260924061635_openai_provider. The legacy shape permits a
-- strict superset of the tightened rows, so re-adding it can never fail on
-- data written under the tightened shape, and no row is touched.
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
    OR (provider = 'openai' AND provider_region IS NULL)
);
