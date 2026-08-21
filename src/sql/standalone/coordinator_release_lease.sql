DELETE FROM standalone_coordinator_leases
WHERE lease_key = ?
  AND owner_id = ?
