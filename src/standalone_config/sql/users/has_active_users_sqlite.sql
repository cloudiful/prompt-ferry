SELECT EXISTS(
    SELECT 1
    FROM standalone_users
    WHERE enabled = 1
) AS has_active;
