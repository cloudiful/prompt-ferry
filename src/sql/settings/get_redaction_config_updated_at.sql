SELECT CASE
    WHEN $1::BOOLEAN THEN (
        SELECT updated_at
        FROM worker_settings
        WHERE setting_key = 'redaction_config'
    )
    ELSE (
        SELECT updated_at
        FROM user_redaction_configs
        WHERE user_id = $2
    )
END AS "updated_at?"
