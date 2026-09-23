-- Issue #277 Phase P7: content half of the request family. A missing row is
-- the new "content expired" signal, so `record_request_record` always writes
-- one row per event even when every payload column is NULL.
INSERT INTO request_record_content(
    created_at,
    event_id,
    request_full_json,
    request_delta_json,
    response_prompt,
    upstream_error_body
)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (event_id, created_at) DO UPDATE SET
    request_full_json = COALESCE(EXCLUDED.request_full_json, request_record_content.request_full_json),
    request_delta_json = COALESCE(EXCLUDED.request_delta_json, request_record_content.request_delta_json),
    response_prompt = COALESCE(EXCLUDED.response_prompt, request_record_content.response_prompt),
    upstream_error_body = COALESCE(EXCLUDED.upstream_error_body, request_record_content.upstream_error_body)
