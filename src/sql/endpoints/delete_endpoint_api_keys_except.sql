DELETE FROM endpoint_api_keys
WHERE endpoint_id = $1
  AND NOT (key_id = ANY($2))
