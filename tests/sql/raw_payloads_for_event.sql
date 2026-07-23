SELECT
    raw.request_raw_json,
    raw.response_raw_body,
    rr.request_conversation_key
FROM request_records rr
LEFT JOIN request_record_raw_payloads raw
  ON raw.event_id = rr.event_id
 AND raw.created_at = rr.created_at
WHERE rr.event_id = $1;
