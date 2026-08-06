UPDATE endpoint_api_keys
SET
    key_label = $3,
    api_key = $4,
    position = $5,
    enabled = $6,
    updated_at = NOW()
WHERE endpoint_id = $1 AND key_id = $2
