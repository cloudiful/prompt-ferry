SELECT key_hash, key_prefix, user_id
FROM client_keys
WHERE enabled = TRUE
ORDER BY key_id ASC
