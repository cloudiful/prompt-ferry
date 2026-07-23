DELETE FROM client_keys
WHERE user_id = $1
  AND key_id = $2
