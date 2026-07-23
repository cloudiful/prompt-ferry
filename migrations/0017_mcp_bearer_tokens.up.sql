ALTER TABLE mcp_servers
ADD COLUMN IF NOT EXISTS bearer_tokens_json JSONB NOT NULL DEFAULT '[]'::JSONB;

UPDATE mcp_servers
SET bearer_tokens_json = CASE
    WHEN COALESCE(BTRIM(bearer_token), '') = '' THEN '[]'::JSONB
    ELSE jsonb_build_array(BTRIM(bearer_token))
END;

ALTER TABLE mcp_servers
DROP COLUMN IF EXISTS bearer_token;
