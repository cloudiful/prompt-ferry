UPDATE mcp_credentials
SET last_error = $2,
    last_error_at = $3,
    updated_at = NOW()
WHERE credential_id = $1
