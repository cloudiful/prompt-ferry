INSERT OR IGNORE INTO standalone_users(
    user_id,
    login_name,
    display_name,
    password_hash,
    is_admin,
    enabled
)
VALUES ($1, 'legacy_user_' || CAST($1 AS TEXT), 'Legacy user ' || CAST($1 AS TEXT), '!', 0, 0);
