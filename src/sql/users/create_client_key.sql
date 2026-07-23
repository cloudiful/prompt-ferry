INSERT INTO client_keys(user_id, label, key_prefix, key_hash, secret)
VALUES ($1, $2, $3, $4, $5)
RETURNING key_id, user_id, key_prefix, label, enabled, last_used_at, created_at, secret
