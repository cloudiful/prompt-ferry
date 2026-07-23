SELECT key_id, user_id, key_prefix, label, enabled, last_used_at, created_at, secret
FROM client_keys
WHERE user_id = $1
ORDER BY created_at DESC
