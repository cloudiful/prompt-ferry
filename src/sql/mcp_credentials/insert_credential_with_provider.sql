INSERT INTO mcp_credentials (
    server_id, credential_label, secret, position, enabled, provider_kind
)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING credential_id, server_id, credential_label, secret, position, enabled,
          provider_kind, default_cost, strict_mode,
          billing_period_start, billing_period_end, provider_remaining, provider_synced_at,
          provider_reset_at, cooldown_until, last_error, last_error_at, created_at, updated_at
