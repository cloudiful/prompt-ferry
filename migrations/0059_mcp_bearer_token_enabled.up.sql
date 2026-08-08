UPDATE mcp_servers
SET bearer_tokens_json = COALESCE(
    (
        SELECT jsonb_agg(
            CASE
                WHEN jsonb_typeof(token.value) = 'string' THEN jsonb_build_object(
                    'token', token.value #>> '{}',
                    'enabled', TRUE
                )
                ELSE token.value
            END
            ORDER BY token.ord
        )
        FROM jsonb_array_elements(bearer_tokens_json) WITH ORDINALITY AS token(value, ord)
    ),
    '[]'::JSONB
);
