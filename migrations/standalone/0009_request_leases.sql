-- Phase 1C-c: durable standalone request leases.
--
-- The standalone SQLite worker persists one lease row per in-flight
-- request so that the request-lease loop introduced in Phase 0 can keep
-- working without depending on the PostgreSQL request-record table
-- (which remains gated and out of scope for this slice). The lease row
-- is keyed by the request id; a heartbeat refresh moves the expiry
-- forward, and an expired row may be taken over by another worker
-- instance, consistent with the existing
-- `standalone_coordinator_leases` semantics and the plan's request-lease
-- requirement.
--
-- Because standalone request records do not yet exist, the stale
-- reconciler only deletes expired lease rows; it does not claim to
-- abort a durable request. Release and refresh operations remain
-- owner-checked so an older process can never delete or mutate a
-- newer owner's row.
--
-- Timestamps are stored as unix-second integers to match the existing
-- `standalone_coordinator_leases` shape; the lease acquisition SQL
-- uses the `updated_at` column as the "now" snapshot so the
-- `expires_at <= updated_at` take-over predicate can be evaluated
-- against the bind-time timestamp without relying on the
-- CURRENT_TIMESTAMP default at acquire time.

CREATE TABLE IF NOT EXISTS standalone_request_leases (
    request_id TEXT PRIMARY KEY,
    owner_worker_id TEXT NOT NULL,
    lease_expires_at INTEGER NOT NULL,
    last_heartbeat_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (lease_expires_at > 0),
    CHECK (last_heartbeat_at > 0),
    CHECK (updated_at > 0)
);

CREATE INDEX IF NOT EXISTS idx_standalone_request_leases_expiry
    ON standalone_request_leases(lease_expires_at);
CREATE INDEX IF NOT EXISTS idx_standalone_request_leases_owner
    ON standalone_request_leases(owner_worker_id);

UPDATE standalone_schema_meta
SET schema_version = 9
WHERE schema_key = 'standalone';