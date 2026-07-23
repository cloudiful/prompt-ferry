INSERT INTO users(login_name, password_hash, display_name, is_admin)
VALUES ($1, $2, $3, TRUE)
