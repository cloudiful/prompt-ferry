UPDATE endpoint_api_keys
SET position = position + $2
WHERE endpoint_id = $1
