INSERT INTO standalone_users(login_name, password_hash, display_name, is_admin, enabled)
VALUES ($1, $2, $1, 1, 1);
