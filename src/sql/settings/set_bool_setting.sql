INSERT INTO worker_settings(setting_key, setting_value)
VALUES ($1, to_jsonb($2::BOOLEAN))
ON CONFLICT (setting_key) DO UPDATE
SET setting_value = EXCLUDED.setting_value, updated_at = NOW()
