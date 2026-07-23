SELECT user_id, login_name, display_name, is_admin, is_active, created_at, updated_at
FROM users
WHERE user_id = $1
  AND is_active = TRUE
