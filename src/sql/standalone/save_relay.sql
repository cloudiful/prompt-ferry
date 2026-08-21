INSERT INTO standalone_relays (
    relay_id, name, relay_url, enabled, tls_mode, bridge_encryption_mode,
    relay_ca_ciphertext, relay_ca_nonce, relay_ca_key_version,
    client_cert_ciphertext, client_cert_nonce, client_cert_key_version,
    client_key_ciphertext, client_key_nonce, client_key_key_version,
    bridge_encryption_key_ciphertext, bridge_encryption_key_nonce,
    bridge_encryption_key_key_version, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(relay_id) DO UPDATE SET
    name = excluded.name,
    relay_url = excluded.relay_url,
    enabled = excluded.enabled,
    tls_mode = excluded.tls_mode,
    bridge_encryption_mode = excluded.bridge_encryption_mode,
    relay_ca_ciphertext = excluded.relay_ca_ciphertext,
    relay_ca_nonce = excluded.relay_ca_nonce,
    relay_ca_key_version = excluded.relay_ca_key_version,
    client_cert_ciphertext = excluded.client_cert_ciphertext,
    client_cert_nonce = excluded.client_cert_nonce,
    client_cert_key_version = excluded.client_cert_key_version,
    client_key_ciphertext = excluded.client_key_ciphertext,
    client_key_nonce = excluded.client_key_nonce,
    client_key_key_version = excluded.client_key_key_version,
    bridge_encryption_key_ciphertext = excluded.bridge_encryption_key_ciphertext,
    bridge_encryption_key_nonce = excluded.bridge_encryption_key_nonce,
    bridge_encryption_key_key_version = excluded.bridge_encryption_key_key_version,
    updated_at = CURRENT_TIMESTAMP;
