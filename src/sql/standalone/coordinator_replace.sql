UPDATE standalone_coordinator_values
SET payload = ?, expires_at = ?, updated_at = ?
WHERE namespace = ?
  AND value_key = ?
  AND payload = ?
  AND expires_at > ?
