INSERT INTO request_record_raw_payloads (
    event_id,
    created_at,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
)
VALUES (
    $1,
    $2,
    'p8-raw-' || ($1::BIGINT)::text,
    1,
    'deadbeef',
    ($2::TIMESTAMPTZ) + INTERVAL '3 days'
)
ON CONFLICT (created_at, event_id) DO NOTHING;
