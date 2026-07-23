UPDATE users
SET
    display_name = COALESCE($2, display_name),
    is_admin = COALESCE($3, is_admin),
    is_active = COALESCE($4, is_active),
    updated_at = NOW()
WHERE user_id = $1
RETURNING user_id, login_name, display_name, is_admin, is_active, created_at, updated_at
