INSERT INTO standalone_coordinator_values(
    namespace,
    value_key,
    payload,
    expires_at,
    updated_at
)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT(namespace, value_key) DO UPDATE SET
    payload = excluded.payload,
    expires_at = excluded.expires_at,
    updated_at = excluded.updated_at
