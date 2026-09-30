-- Issue #661 Phase P3: administrator configuration archive audit trail.
--
-- One row per real export/import attempt; the read-only import preview writes
-- nothing. The actor is nullable and cleared when the account is deleted so the
-- trail outlives the operator. Only non-secret metadata is stored: the
-- passphrase, the archive bytes, secret values, and raw persistence errors
-- never reach this table (`error_message` carries a redacted, bounded summary).
CREATE TABLE IF NOT EXISTS config_archive_audit (
    audit_id BIGSERIAL PRIMARY KEY,
    actor_user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    action TEXT NOT NULL CHECK (action IN ('export', 'import')),
    backend_kind TEXT NOT NULL,
    format_version INTEGER,
    archive_bytes BIGINT,
    payload_fingerprint TEXT,
    success BOOLEAN NOT NULL,
    error_code TEXT,
    error_message TEXT,
    domain_summary JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_config_archive_audit_created_at
    ON config_archive_audit (created_at DESC, audit_id DESC);
