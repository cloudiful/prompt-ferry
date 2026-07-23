SELECT COUNT(*)::BIGINT AS "count!"
FROM client_keys
WHERE user_id = $1
