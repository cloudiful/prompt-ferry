INSERT INTO standalone_coordinator_values(
    namespace,
    value_key,
    payload,
    expires_at,
    updated_at
)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT(namespace, value_key) DO NOTHING
