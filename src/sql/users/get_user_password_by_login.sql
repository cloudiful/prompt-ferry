SELECT user_id, login_name, password_hash, display_name, is_admin, is_active
FROM users
WHERE login_name = $1
