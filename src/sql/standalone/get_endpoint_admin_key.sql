SELECT admin_api_key_ciphertext, admin_api_key_nonce, admin_api_key_key_version
FROM standalone_endpoint_admin_keys
WHERE endpoint_id = ?;
