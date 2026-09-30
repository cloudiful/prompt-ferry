-- Issue #661 Phase P3: administrator configuration archive audit trail.
--
-- Mirrors the PostgreSQL `config_archive_audit` contract: one row per real
-- export/import attempt, nullable actor cleared with the account, and only
-- non-secret metadata (passphrase, archive bytes, secret values, and raw
-- persistence errors are never stored). `domain_summary` holds the canonical
-- JSON array of per-domain record counts.
CREATE TABLE IF NOT EXISTS standalone_config_archive_audit (
    audit_id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_user_id INTEGER REFERENCES standalone_users(user_id) ON DELETE SET NULL,
    action TEXT NOT NULL CHECK (action IN ('export', 'import')),
    backend_kind TEXT NOT NULL,
    format_version INTEGER,
    archive_bytes INTEGER,
    payload_fingerprint TEXT,
    success INTEGER NOT NULL CHECK (success IN (0, 1)),
    error_code TEXT,
    error_message TEXT,
    domain_summary TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_standalone_config_archive_audit_created_at
    ON standalone_config_archive_audit (created_at DESC, audit_id DESC);

UPDATE standalone_schema_meta
SET schema_version = 36
WHERE schema_key = 'standalone';
