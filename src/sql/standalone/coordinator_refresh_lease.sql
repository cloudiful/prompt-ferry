UPDATE standalone_coordinator_leases
SET expires_at = ?, updated_at = ?
WHERE lease_key = ?
  AND owner_id = ?
  AND expires_at > ?
