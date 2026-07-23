DELETE FROM request_records
WHERE created_at < NOW() - ($1::BIGINT * INTERVAL '1 day')
