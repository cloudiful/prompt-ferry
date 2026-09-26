-- Issue #599 R2a: per-endpoint ChatGPT subscription OAuth token, server-side
-- only (never echoed in logs or responses). Plaintext on PostgreSQL mirrors
-- the existing `api_key` asymmetry (SQLite uses the envelope triplets in
-- standalone 0033). The token columns are NULLABLE by design: storing NULLs
-- clears the credential (the derived plan falls back to `platform_api_key`);
-- presence means `refresh_token IS NOT NULL`. `ON DELETE CASCADE` drops the
-- token with its endpoint so no orphaned secret survives endpoint deletion.
CREATE TABLE IF NOT EXISTS endpoint_oauth_tokens (
    endpoint_id UUID PRIMARY KEY REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    access_token TEXT,
    refresh_token TEXT,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
