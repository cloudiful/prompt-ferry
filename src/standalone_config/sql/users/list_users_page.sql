SELECT user_id,
       login_name,
       display_name,
       is_admin,
       enabled AS is_active,
       created_at,
       updated_at
FROM standalone_users
ORDER BY updated_at DESC, user_id DESC
LIMIT $2 OFFSET $1;
