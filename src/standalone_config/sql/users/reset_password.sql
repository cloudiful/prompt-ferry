UPDATE standalone_users
SET password_hash = $2,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE user_id = $1;
