ALTER TABLE mcp_servers
ADD COLUMN IF NOT EXISTS bearer_token TEXT NOT NULL DEFAULT '';

UPDATE mcp_servers
SET bearer_token = COALESCE(
    (
        SELECT value
        FROM jsonb_array_elements_text(bearer_tokens_json) WITH ORDINALITY AS token(value, ord)
        ORDER BY ord
        LIMIT 1
    ),
    ''
);

ALTER TABLE mcp_servers
DROP COLUMN IF EXISTS bearer_tokens_json;
