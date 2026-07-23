INSERT INTO user_redaction_configs(user_id, config, updated_at)
VALUES ($1, $2, now())
ON CONFLICT (user_id)
DO UPDATE SET config = EXCLUDED.config, updated_at = now()
