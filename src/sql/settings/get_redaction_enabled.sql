SELECT setting_value = 'true'::JSONB AS "enabled!"
FROM worker_settings
WHERE setting_key = 'redaction_enabled'
