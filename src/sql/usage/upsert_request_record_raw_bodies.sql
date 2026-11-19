-- Partial capture writes merge field-by-field: a request-only write followed
-- by a response-only write keeps both bodies for the same event.
INSERT INTO request_record_raw_payloads(
    event_id,
    created_at,
    request_raw_json,
    response_raw_body
)
VALUES (
    $1,
    $2,
    $3,
    $4
)
ON CONFLICT (created_at, event_id)
DO UPDATE SET
    request_raw_json = COALESCE(
        EXCLUDED.request_raw_json,
        request_record_raw_payloads.request_raw_json
    ),
    response_raw_body = COALESCE(
        EXCLUDED.response_raw_body,
        request_record_raw_payloads.response_raw_body
    );
