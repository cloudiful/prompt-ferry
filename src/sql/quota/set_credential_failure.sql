UPDATE mcp_credentials
SET cooldown_until = $2,
    last_error = $3,
    last_error_at = $4,
    updated_at = NOW()
WHERE credential_id = $1
