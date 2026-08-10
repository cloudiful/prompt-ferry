SELECT credential_id, server_id, credential_label, secret, position, enabled, quota_group_id,
       provider_kind, daily_limit, monthly_limit, default_cost, strict_mode,
       billing_period_start, billing_period_end, provider_remaining, provider_synced_at,
       provider_reset_at, cooldown_until, last_error, last_error_at, created_at, updated_at
FROM mcp_credentials
WHERE server_id = $1
ORDER BY position ASC, credential_id ASC
