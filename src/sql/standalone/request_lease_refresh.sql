UPDATE standalone_request_leases
SET lease_expires_at = ?, last_heartbeat_at = ?, updated_at = ?
WHERE request_id = ?
  AND owner_worker_id = ?
  AND lease_expires_at > ?;