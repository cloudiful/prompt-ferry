DELETE FROM request_records WHERE request_id = ANY($1::UUID[]) AND user_id = ANY($2::BIGINT[])
