SELECT key_id, endpoint_id, key_label, api_key, position, enabled, created_at, updated_at
FROM endpoint_api_keys
WHERE endpoint_id = ANY($1)
ORDER BY endpoint_id ASC, position ASC, created_at ASC
