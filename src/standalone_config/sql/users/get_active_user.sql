SELECT user_id,
       login_name,
       display_name,
       is_admin,
       enabled AS is_active,
       created_at,
       updated_at
FROM standalone_users
WHERE user_id = $1
  AND enabled = 1;
