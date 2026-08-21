SELECT key_id, user_id, key_prefix, label, enabled,
       secret_ciphertext, secret_nonce, secret_key_version
FROM standalone_client_keys
WHERE user_id = ?
ORDER BY key_id
LIMIT ? OFFSET ?;