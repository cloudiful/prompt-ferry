UPDATE endpoint_api_keys
SET position = position + $3
WHERE endpoint_id = $1 AND key_id = ANY($2)
