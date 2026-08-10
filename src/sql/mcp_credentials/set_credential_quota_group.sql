UPDATE mcp_credentials
SET quota_group_id = $2,
    updated_at = NOW()
WHERE credential_id = $1
RETURNING credential_id
