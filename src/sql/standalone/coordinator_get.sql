SELECT payload
FROM standalone_coordinator_values
WHERE namespace = ?
  AND value_key = ?
  AND expires_at > ?
