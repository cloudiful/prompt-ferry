UPDATE users
SET password_hash = $2, updated_at = NOW()
WHERE user_id = $1
