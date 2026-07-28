INSERT INTO request_record_leases (
    request_id,
    owner_worker_id,
    lease_expires_at,
    last_heartbeat_at
)
VALUES ($1, NULL, $2, $3)
ON CONFLICT (request_id) DO UPDATE
SET lease_expires_at = EXCLUDED.lease_expires_at,
    last_heartbeat_at = EXCLUDED.last_heartbeat_at,
    updated_at = NOW();
