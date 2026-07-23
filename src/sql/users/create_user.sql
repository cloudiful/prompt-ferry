INSERT INTO users(login_name, password_hash, display_name, is_admin)
VALUES ($1, $2, $3, $4)
RETURNING user_id, login_name, display_name, is_admin, is_active, created_at, updated_at
