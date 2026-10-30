INSERT INTO users (login_name, password_hash, display_name)
VALUES ($1, 'fixture-not-a-password', $1)
RETURNING user_id AS "user_id!"
