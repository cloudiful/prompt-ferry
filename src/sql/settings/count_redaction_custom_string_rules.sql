WITH selected_config AS (
    SELECT CASE
        WHEN $1::BOOLEAN THEN COALESCE(
            (SELECT setting_value
             FROM worker_settings
             WHERE setting_key = 'redaction_config'),
            '{"custom_strings":[]}'::jsonb
        )
        ELSE COALESCE(
            (SELECT config
             FROM user_redaction_configs
             WHERE user_id = $2),
            '{"custom_strings":[]}'::jsonb
        )
    END AS config
)
SELECT COUNT(elem.value)::BIGINT AS "total!"
FROM selected_config
LEFT JOIN LATERAL jsonb_array_elements(
    COALESCE(config -> 'custom_strings', '[]'::jsonb)
) AS elem(value)
    ON $3::TEXT IS NULL
    OR BTRIM($3::TEXT) = ''
    OR LOWER(elem.value ->> 'pattern') LIKE '%' || LOWER(BTRIM($3::TEXT)) || '%';
