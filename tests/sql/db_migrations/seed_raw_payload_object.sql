-- Raw payload metadata row exactly as the write path persists it
-- (object-store backed). `created_at` decides the routing: inside a daily
-- partition's bounds it lands there, otherwise the `_default` partition
-- takes it.
INSERT INTO request_record_raw_payloads (
    event_id,
    created_at,
    raw_object_key,
    raw_object_size_bytes,
    raw_object_sha256,
    raw_object_expires_at
)
VALUES ($1, $2, $3, $4, $5, $6)
