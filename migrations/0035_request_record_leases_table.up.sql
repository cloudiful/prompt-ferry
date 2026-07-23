DROP INDEX IF EXISTS idx_request_records_active_lease;

CREATE TABLE IF NOT EXISTS request_record_leases (
    request_id UUID PRIMARY KEY,
    owner_worker_id UUID,
    lease_expires_at TIMESTAMPTZ NOT NULL,
    last_heartbeat_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_request_record_leases_expires_at
ON request_record_leases(lease_expires_at ASC);

INSERT INTO request_record_leases (
    request_id,
    owner_worker_id,
    lease_expires_at,
    last_heartbeat_at
)
SELECT
    request_id,
    owner_worker_id,
    lease_expires_at,
    COALESCE(last_heartbeat_at, updated_at)
FROM request_records
WHERE event_kind = 'request'
  AND request_state IN ('received', 'awaiting_approval', 'upstream_processing')
  AND request_id IS NOT NULL
  AND lease_expires_at IS NOT NULL
ON CONFLICT (request_id) DO NOTHING;
