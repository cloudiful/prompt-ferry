SELECT user_id, login_name, display_name, is_admin, is_active, created_at, updated_at
FROM users
ORDER BY updated_at DESC, user_id DESC
LIMIT $2 OFFSET $1
