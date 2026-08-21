SELECT relay_id, name, relay_url, enabled, tls_mode, bridge_encryption_mode,
       relay_ca_ciphertext, relay_ca_nonce, relay_ca_key_version,
       client_cert_ciphertext, client_cert_nonce, client_cert_key_version,
       client_key_ciphertext, client_key_nonce, client_key_key_version,
       bridge_encryption_key_ciphertext, bridge_encryption_key_nonce,
       bridge_encryption_key_key_version
FROM standalone_relays
ORDER BY name
LIMIT ? OFFSET ?;