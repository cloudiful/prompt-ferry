SELECT endpoint_id
FROM endpoint_oauth_tokens
WHERE refresh_token IS NOT NULL
