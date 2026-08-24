DELETE FROM standalone_request_leases
WHERE request_id = ?
  AND owner_worker_id = ?;