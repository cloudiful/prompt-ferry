SELECT endpoint_id AS "endpoint_id?"
FROM user_endpoint_settings
WHERE user_id = $1
