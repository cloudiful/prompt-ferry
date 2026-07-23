SELECT setting_value AS "setting_value!"
FROM worker_settings
WHERE setting_key = $1
