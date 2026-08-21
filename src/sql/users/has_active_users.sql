SELECT EXISTS(
    SELECT 1
    FROM users
    WHERE is_active = TRUE
) AS "has_active!";
