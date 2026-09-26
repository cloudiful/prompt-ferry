INSERT INTO endpoint_oauth_tokens (endpoint_id, access_token, refresh_token, expires_at, created_at, updated_at)
VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (endpoint_id) DO UPDATE SET
    access_token = EXCLUDED.access_token,
    refresh_token = EXCLUDED.refresh_token,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW();
