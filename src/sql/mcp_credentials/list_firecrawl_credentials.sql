SELECT credential_id, secret
FROM mcp_credentials
WHERE enabled = TRUE AND provider_kind = 'firecrawl'
ORDER BY credential_id
