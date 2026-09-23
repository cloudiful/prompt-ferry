-- Row parked in the overflow staging table, the mid-staging state the raw
-- partition manager produces while a daily partition is being created.
INSERT INTO request_record_raw_payloads_overflow (
    event_id,
    created_at,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
)
VALUES ($1, $2, $3, $4, $5, $6)
