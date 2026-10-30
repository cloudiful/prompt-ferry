INSERT INTO request_records
    (request_id, user_id, path, created_at, request_category, mcp_server_name,
     ok, request_state, input_tokens, cache_read_tokens, cache_write_tokens,
     output_tokens, total_tokens, duration_ms, ttft_ms)
VALUES ($1, $2, '/p3t-fixture', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
