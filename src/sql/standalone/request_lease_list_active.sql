SELECT request_id, owner_worker_id, lease_expires_at, last_heartbeat_at, updated_at
FROM standalone_request_leases
WHERE lease_expires_at > ?
ORDER BY lease_expires_at ASC;