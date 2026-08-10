UPDATE mcp_credentials
SET provider_remaining = $2,
    provider_synced_at = $3,
    provider_reset_at = $4,
    updated_at = NOW()
WHERE credential_id = $1
