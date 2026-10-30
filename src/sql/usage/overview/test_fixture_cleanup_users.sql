DELETE FROM users WHERE user_id = ANY($1::BIGINT[]) AND login_name = ANY($2::TEXT[])
