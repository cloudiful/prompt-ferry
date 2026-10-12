UPDATE endpoint_api_keys
SET position = $3
WHERE endpoint_id = $1 AND key_id = $2
