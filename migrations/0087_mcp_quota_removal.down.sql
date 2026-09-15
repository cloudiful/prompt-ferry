-- Issue #414 Phase 2 rollback: restore the quota ledger schema without
-- backfilling data. Credentials keep their rows; the group link is nullable.
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
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS mcp_quota_accounts (
    account_id BIGSERIAL PRIMARY KEY,
    group_id UUID NOT NULL REFERENCES mcp_quota_groups(group_id) ON DELETE CASCADE,
    period_kind TEXT NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    used_units DOUBLE PRECISION NOT NULL DEFAULT 0,
    reserved_units DOUBLE PRECISION NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE mcp_credentials ADD COLUMN IF NOT EXISTS quota_group_id UUID REFERENCES mcp_quota_groups(group_id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_mcp_credentials_group ON mcp_credentials(quota_group_id);
