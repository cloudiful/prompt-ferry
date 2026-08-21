INSERT INTO standalone_settings(setting_key, value_version, value_json, updated_at)
VALUES (?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(setting_key) DO UPDATE SET
    value_version = excluded.value_version,
    value_json = excluded.value_json,
    updated_at = CURRENT_TIMESTAMP;
