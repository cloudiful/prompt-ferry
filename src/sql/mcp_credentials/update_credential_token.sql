UPDATE mcp_credentials
SET credential_label = $2,
    secret = $3,
    enabled = $4,
    updated_at = NOW()
WHERE credential_id = $1
RETURNING credential_id, server_id, credential_label, secret, position, enabled, quota_group_id,
          provider_kind, daily_limit, monthly_limit, default_cost, strict_mode,
          billing_period_start, billing_period_end, provider_remaining, provider_synced_at,
          provider_reset_at, cooldown_until, last_error, last_error_at, created_at, updated_at
