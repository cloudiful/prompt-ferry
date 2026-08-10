CREATE TABLE IF NOT EXISTS mcp_quota_groups (
    group_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    name TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT 'admin',
    owner_user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    provider_kind TEXT,
    unit TEXT NOT NULL DEFAULT 'requests',
    daily_limit DOUBLE PRECISION,
    monthly_limit DOUBLE PRECISION,
    default_cost DOUBLE PRECISION NOT NULL DEFAULT 1,
    strict_mode BOOLEAN NOT NULL DEFAULT FALSE,
    billing_period_start TIMESTAMPTZ,
    billing_period_end TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_mcp_quota_groups_unit CHECK (unit IN ('requests', 'credits')),
    CONSTRAINT ck_mcp_quota_groups_default_cost CHECK (default_cost >= 0),
    CONSTRAINT ck_mcp_quota_groups_limits CHECK (
        (daily_limit IS NULL OR daily_limit >= 0)
        AND (monthly_limit IS NULL OR monthly_limit >= 0)
    ),
    CONSTRAINT ck_mcp_quota_groups_period CHECK (
        billing_period_end IS NULL
        OR billing_period_start IS NULL
        OR billing_period_end > billing_period_start
    )
);

CREATE TABLE IF NOT EXISTS mcp_credentials (
    credential_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    server_id UUID NOT NULL REFERENCES mcp_servers(server_id) ON DELETE CASCADE,
    credential_label TEXT NOT NULL,
    secret TEXT NOT NULL,
    position INTEGER NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    quota_group_id UUID REFERENCES mcp_quota_groups(group_id) ON DELETE SET NULL,
    provider_kind TEXT,
    daily_limit DOUBLE PRECISION,
    monthly_limit DOUBLE PRECISION,
    default_cost DOUBLE PRECISION NOT NULL DEFAULT 1,
    strict_mode BOOLEAN NOT NULL DEFAULT FALSE,
    billing_period_start TIMESTAMPTZ,
    billing_period_end TIMESTAMPTZ,
    provider_remaining DOUBLE PRECISION,
    provider_synced_at TIMESTAMPTZ,
    provider_reset_at TIMESTAMPTZ,
    cooldown_until TIMESTAMPTZ,
    last_error TEXT,
    last_error_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_mcp_credentials_default_cost CHECK (default_cost >= 0),
    CONSTRAINT ck_mcp_credentials_limits CHECK (
        (daily_limit IS NULL OR daily_limit >= 0)
        AND (monthly_limit IS NULL OR monthly_limit >= 0)
    ),
    CONSTRAINT ck_mcp_credentials_period CHECK (
        billing_period_end IS NULL
        OR billing_period_start IS NULL
        OR billing_period_end > billing_period_start
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_credentials_server_position
ON mcp_credentials(server_id, position);

CREATE INDEX IF NOT EXISTS idx_mcp_credentials_group
ON mcp_credentials(quota_group_id);

CREATE TABLE IF NOT EXISTS mcp_quota_accounts (
    account_id BIGSERIAL PRIMARY KEY,
    group_id UUID NOT NULL REFERENCES mcp_quota_groups(group_id) ON DELETE CASCADE,
    period_kind TEXT NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    used_units DOUBLE PRECISION NOT NULL DEFAULT 0,
    reserved_units DOUBLE PRECISION NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_mcp_quota_accounts_units CHECK (used_units >= 0 AND reserved_units >= 0),
    CONSTRAINT ck_mcp_quota_accounts_period_kind CHECK (period_kind IN ('day', 'month')),
    CONSTRAINT uq_mcp_quota_accounts_group_period UNIQUE (group_id, period_kind, period_start)
);

CREATE TABLE IF NOT EXISTS mcp_quota_reservations (
    reservation_id BIGSERIAL PRIMARY KEY,
    day_account_id BIGINT REFERENCES mcp_quota_accounts(account_id) ON DELETE CASCADE,
    month_account_id BIGINT REFERENCES mcp_quota_accounts(account_id) ON DELETE CASCADE,
    credential_id UUID REFERENCES mcp_credentials(credential_id) ON DELETE SET NULL,
    request_id UUID NOT NULL,
    units DOUBLE PRECISION NOT NULL,
    status TEXT NOT NULL DEFAULT 'reserved',
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    committed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_mcp_quota_reservations_units CHECK (units >= 0),
    CONSTRAINT ck_mcp_quota_reservations_status CHECK (status IN ('reserved', 'committed', 'released')),
    CONSTRAINT ck_mcp_quota_reservations_account CHECK (day_account_id IS NOT NULL OR month_account_id IS NOT NULL)
);

CREATE INDEX IF NOT EXISTS idx_mcp_quota_reservations_request
ON mcp_quota_reservations(request_id);

CREATE INDEX IF NOT EXISTS idx_mcp_quota_reservations_expiry
ON mcp_quota_reservations(status, expires_at);

-- 回填：每个 MCP server 一个默认 quota group（继承 server 的日/月请求预算）。
INSERT INTO mcp_quota_groups (group_id, name, scope, provider_kind, unit, daily_limit, monthly_limit)
SELECT
    md5('group:' || server_id::text)::uuid,
    name || ' default',
    'admin',
    NULL,
    'requests',
    daily_max_requests,
    monthly_max_requests
FROM mcp_servers
ON CONFLICT DO NOTHING;

-- 回填：bearer_tokens_json 数组展开为 mcp_credentials，并入默认 group。
-- 默认 group 通过 group_id 公式匹配（同 server 名的 server 不会互相串绑）。
INSERT INTO mcp_credentials (server_id, credential_label, secret, position, enabled, quota_group_id)
SELECT
    m.server_id,
    'token-' || m.ord,
    parsed.token,
    m.ord,
    parsed.enabled,
    md5('group:' || m.server_id::text)::uuid
FROM (
    SELECT
        s.server_id,
        t.elem,
        t.ord - 1 AS ord
    FROM mcp_servers s,
    LATERAL jsonb_array_elements(s.bearer_tokens_json) WITH ORDINALITY AS t(elem, ord)
) m
CROSS JOIN LATERAL (
    SELECT
        CASE
            WHEN m.elem ? 'token' THEN COALESCE(m.elem->>'token', '')
            ELSE m.elem #>> '{}'
        END AS token,
        CASE
            WHEN m.elem ? 'token' THEN COALESCE((m.elem->>'enabled')::boolean, TRUE)
            ELSE TRUE
        END AS enabled
) parsed
WHERE parsed.token <> ''
ON CONFLICT (server_id, position) DO NOTHING;
