UPDATE mcp_oauth_credentials AS credential
SET credentials_ciphertext = $6,
    credentials_nonce = $7,
    credentials_key_version = $8,
    scopes = $9,
    expires_at = $10,
    issuer = $11,
    generation = credential.generation + 1,
    updated_at = NOW()
WHERE credential.server_id = $1
  AND credential.owner_user_id = $2
  AND credential.connection_id = $3
  AND credential.generation = $4
  AND credential.config_fingerprint = $5
  AND EXISTS (
      SELECT 1
      FROM mcp_oauth_leases AS lease
      WHERE lease.server_id = credential.server_id
        AND lease.owner_user_id = credential.owner_user_id
        AND lease.connection_id = credential.connection_id
        AND lease.generation = credential.generation
        AND lease.lease_owner = $12
        AND lease.expires_at > NOW()
  )
RETURNING server_id,
    owner_user_id,
    connection_id,
    generation,
    config_fingerprint,
    registration_ciphertext,
    registration_nonce,
    registration_key_version,
    credentials_ciphertext,
    credentials_nonce,
    credentials_key_version,
    scopes,
    expires_at,
    issuer,
    created_at,
    updated_at
