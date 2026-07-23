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
),
rules AS (
    SELECT
        elem.value AS rule,
        elem.ordinality::BIGINT - 1 AS array_index
    FROM selected_config,
         LATERAL jsonb_array_elements(
             COALESCE(config -> 'custom_strings', '[]'::jsonb)
         ) WITH ORDINALITY AS elem(value, ordinality)
    WHERE $5::TEXT IS NULL
       OR BTRIM($5::TEXT) = ''
       OR LOWER(elem.value ->> 'pattern') LIKE '%' || LOWER(BTRIM($5::TEXT)) || '%'
)
SELECT
    rule ->> 'pattern' AS "pattern!",
    rule ->> 'match_type' AS "match_type!",
    rule ->> 'scope' AS "scope!",
    array_index AS "array_index!"
FROM rules
ORDER BY array_index DESC
OFFSET $3
LIMIT $4;
