-- The raw payload reference shares the request record's partition key, so the
-- canonical `created_at` is bound directly. Selecting it back from
-- `request_records` would scan every daily partition of the request family.
INSERT INTO request_record_raw_payloads(
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
    $3,
    $4,
    $5,
    $6
)
ON CONFLICT (created_at, event_id)
DO UPDATE SET
    raw_object_key = EXCLUDED.raw_object_key,
    raw_object_size_bytes = EXCLUDED.raw_object_size_bytes,
    raw_object_sha256 = EXCLUDED.raw_object_sha256,
    raw_object_expires_at = EXCLUDED.raw_object_expires_at
