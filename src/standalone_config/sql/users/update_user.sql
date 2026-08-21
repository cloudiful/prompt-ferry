UPDATE standalone_users
SET display_name = COALESCE($2, display_name),
    is_admin = COALESCE($3, is_admin),
    enabled = COALESCE($4, enabled),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE user_id = $1
RETURNING user_id,
          login_name,
          display_name,
          is_admin,
          enabled AS is_active,
          created_at,
          updated_at;
