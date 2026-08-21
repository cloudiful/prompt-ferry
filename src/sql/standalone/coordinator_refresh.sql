UPDATE standalone_coordinator_values
SET expires_at = ?, updated_at = ?
WHERE namespace = ?
  AND value_key = ?
  AND payload = ?
  AND expires_at > ?
