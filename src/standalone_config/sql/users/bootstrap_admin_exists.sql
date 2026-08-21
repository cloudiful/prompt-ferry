SELECT EXISTS(
    SELECT 1
    FROM standalone_users
    WHERE login_name = $1
) AS "exists";
