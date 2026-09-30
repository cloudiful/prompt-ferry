INSERT INTO standalone_users(
    user_id, login_name, password_hash, display_name, is_admin, enabled, created_at, updated_at
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8);
