UPDATE client_keys
SET label = COALESCE($3, label), enabled = COALESCE($4, enabled)
WHERE user_id = $1
  AND key_id = $2
RETURNING key_id, user_id, key_prefix, label, enabled, last_used_at, created_at
       , secret
