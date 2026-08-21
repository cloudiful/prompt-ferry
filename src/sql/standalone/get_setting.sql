SELECT setting_key, value_version, value_json
FROM standalone_settings
WHERE setting_key = ?;