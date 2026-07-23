INSERT INTO worker_settings(setting_key, setting_value)
VALUES ('redaction_enabled', to_jsonb($1::BOOLEAN))
ON CONFLICT (setting_key) DO UPDATE
SET setting_value = EXCLUDED.setting_value, updated_at = NOW()
