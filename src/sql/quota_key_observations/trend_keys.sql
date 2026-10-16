-- Bounded key selection for the quota observation trend: only keys with rows
-- inside the half-open request window, latest observed first with a stable
-- key_id tie-break. The caller requests one row past the display cap so it can
-- report truncation honestly.
SELECT key_id
FROM quota_key_observations
WHERE endpoint_id = $1
  AND observed_at >= $2
  AND observed_at < $3
  AND ($4::UUID IS NULL OR key_id = $4)
GROUP BY key_id
ORDER BY MAX(observed_at) DESC, key_id ASC
LIMIT 25;
