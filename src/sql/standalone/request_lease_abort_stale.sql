-- Stale lease cleanup: delete only expired rows. Because standalone
-- request records do not exist yet, the reconciler cannot claim to
-- abort a durable request; the cleanup simply removes rows whose
-- `lease_expires_at` is at or before the bind-time `now` snapshot.
DELETE FROM standalone_request_leases
WHERE lease_expires_at <= ?;