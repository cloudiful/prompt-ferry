SELECT endpoint_id, access_token, refresh_token, expires_at, created_at, updated_at
FROM endpoint_oauth_tokens
WHERE endpoint_id = $1
