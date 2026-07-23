UPDATE request_records rr
SET
    request_has_previous_response_id = FALSE,
    request_previous_response_id = NULL,
    request_previous_response_parent_found = NULL,
    request_conversation_key = NULL,
    request_conversation_parent_found = NULL
FROM request_record_raw_payloads raw
WHERE raw.created_at < $1
  AND rr.event_id = raw.event_id;
