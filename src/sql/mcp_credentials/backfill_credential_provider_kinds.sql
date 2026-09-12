UPDATE mcp_credentials c
SET provider_kind = CASE
        WHEN s.provider_kind IN ('context7', 'firecrawl', 'minimax') THEN s.provider_kind
        ELSE NULL
    END,
    updated_at = NOW()
FROM mcp_servers s
WHERE c.server_id = s.server_id
  AND c.provider_kind IS DISTINCT FROM CASE
        WHEN s.provider_kind IN ('context7', 'firecrawl', 'minimax') THEN s.provider_kind
        ELSE NULL
    END
