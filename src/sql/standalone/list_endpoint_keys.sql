SELECT key_id, endpoint_id, key_label, enabled, position,
       api_key_ciphertext, api_key_nonce, api_key_key_version
FROM standalone_endpoint_keys
ORDER BY endpoint_id, position;
