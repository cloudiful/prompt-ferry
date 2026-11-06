UPDATE standalone_mcp_oauth_credentials
SET credentials_ciphertext = ?6,
    credentials_nonce = ?7,
    credentials_key_version = ?8,
    scopes = ?9,
    expires_at = ?10,
    issuer = ?11,
    generation = generation + 1,
    updated_at = ?12
WHERE server_id = ?1
  AND owner_user_id = ?2
  AND connection_id = ?3
  AND generation = ?4
  AND config_fingerprint = ?5
  AND EXISTS (
      SELECT 1
      FROM standalone_mcp_oauth_leases
      WHERE server_id = ?1
        AND owner_user_id = ?2
        AND connection_id = ?3
        AND generation = ?4
        AND lease_owner = ?13
        AND expires_at > ?14
  )
