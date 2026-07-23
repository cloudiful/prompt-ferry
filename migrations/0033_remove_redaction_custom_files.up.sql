UPDATE worker_settings
SET setting_value = setting_value - 'custom_files'
WHERE setting_key = 'redaction_config'
  AND setting_value ? 'custom_files';

UPDATE user_redaction_configs
SET config = config - 'custom_files'
WHERE config ? 'custom_files';
