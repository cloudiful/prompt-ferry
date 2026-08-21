INSERT INTO standalone_users(login_name, password_hash, display_name, is_admin, enabled)
VALUES ($1, $2, $3, $4, 1)
RETURNING user_id,
          login_name,
          display_name,
          is_admin,
          enabled AS is_active,
          created_at,
          updated_at;
