INSERT INTO user_endpoint_settings(user_id, endpoint_id)
VALUES ($1, $2)
ON CONFLICT (user_id) DO UPDATE
SET endpoint_id = EXCLUDED.endpoint_id, updated_at = NOW()
