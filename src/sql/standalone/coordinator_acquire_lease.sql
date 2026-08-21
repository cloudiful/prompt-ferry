INSERT INTO standalone_coordinator_leases(
    lease_key,
    owner_id,
    expires_at,
    updated_at
)
VALUES (?, ?, ?, ?)
ON CONFLICT(lease_key) DO UPDATE SET
    owner_id = excluded.owner_id,
    expires_at = excluded.expires_at,
    updated_at = excluded.updated_at
WHERE standalone_coordinator_leases.expires_at <= excluded.updated_at
   OR standalone_coordinator_leases.owner_id = excluded.owner_id
RETURNING owner_id
