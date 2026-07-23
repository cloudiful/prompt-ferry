INSERT INTO endpoint_api_keys(endpoint_id, key_label, api_key, position, enabled)
VALUES ($1, $2, $3, $4, $5)
RETURNING key_id, endpoint_id, key_label, api_key, position, enabled, created_at, updated_at
