INSERT INTO standalone_request_leases(
    request_id,
    owner_worker_id,
    lease_expires_at,
    last_heartbeat_at,
    updated_at
)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT(request_id) DO UPDATE SET
    owner_worker_id = excluded.owner_worker_id,
    lease_expires_at = excluded.lease_expires_at,
    last_heartbeat_at = excluded.last_heartbeat_at,
    updated_at = excluded.updated_at
WHERE standalone_request_leases.lease_expires_at <= excluded.updated_at
   OR standalone_request_leases.owner_worker_id = excluded.owner_worker_id
RETURNING owner_worker_id;