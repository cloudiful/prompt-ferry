SELECT user_id,
       login_name,
       password_hash,
       display_name,
       is_admin,
       enabled AS is_active
FROM standalone_users
WHERE login_name = $1;
