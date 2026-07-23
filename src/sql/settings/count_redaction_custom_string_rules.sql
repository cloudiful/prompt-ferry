WITH selected_config AS (
    SELECT
        CASE
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
        END AS config,
        CASE
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
        END AS updated_at
)
SELECT
    COUNT(elem.value)::BIGINT AS "total!",
    selected_config.updated_at AS "updated_at?"
FROM selected_config
LEFT JOIN LATERAL jsonb_array_elements(
    COALESCE(config -> 'custom_strings', '[]'::jsonb)
) AS elem(value)
    ON $3::TEXT IS NULL
    OR BTRIM($3::TEXT) = ''
    OR LOWER(elem.value ->> 'pattern') LIKE '%' || LOWER(BTRIM($3::TEXT)) || '%'
GROUP BY selected_config.updated_at;
