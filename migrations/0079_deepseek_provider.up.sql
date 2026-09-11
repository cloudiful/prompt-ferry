-- P0 (issue #287): add the DeepSeek upstream provider alongside
-- generic/MiniMax/CommandCode/OpencodeGo/OpenRouter/GLM. DeepSeek endpoints
-- carry no provider_region (like generic, command_code, opencode_go,
-- openrouter and glm) and never gain the MiniMax builtin MCP privilege;
-- region/MCP enforcement lives in the admin handler layer, these CHECKs are
-- the DB backstop.
ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider_region;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider
CHECK (provider IN ('generic', 'minimax', 'command_code', 'opencode_go', 'openrouter', 'glm', 'deepseek'));

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
