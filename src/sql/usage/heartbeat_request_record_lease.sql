INSERT INTO request_record_leases (
    request_id,
    owner_worker_id,
    lease_expires_at,
    last_heartbeat_at
)
SELECT
    $1,
    $2,
    $3,
    $4
FROM request_records
WHERE request_id = $1
  AND event_kind = 'request'
  AND request_state IN ('received', 'awaiting_approval', 'upstream_processing')
ON CONFLICT (request_id) DO UPDATE
SET
    owner_worker_id = COALESCE(EXCLUDED.owner_worker_id, request_record_leases.owner_worker_id),
    lease_expires_at = EXCLUDED.lease_expires_at,
    last_heartbeat_at = EXCLUDED.last_heartbeat_at,
    updated_at = NOW()
