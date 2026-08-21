CREATE TABLE IF NOT EXISTS standalone_coordinator_values (
    namespace TEXT NOT NULL,
    value_key TEXT NOT NULL,
    payload TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, value_key)
);

CREATE INDEX IF NOT EXISTS idx_standalone_coordinator_values_expiry
    ON standalone_coordinator_values(expires_at);

CREATE TABLE IF NOT EXISTS standalone_coordinator_leases (
    lease_key TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_standalone_coordinator_leases_expiry
    ON standalone_coordinator_leases(expires_at);

UPDATE standalone_schema_meta
SET schema_version = 4
WHERE schema_key = 'standalone';
