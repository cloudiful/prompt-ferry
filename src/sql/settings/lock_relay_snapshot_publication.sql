-- Serialize the relay snapshot publication boundary.
--
-- The lock is transaction scoped and is held while the snapshot contents are
-- read and the version is assigned, so two publishers can never label
-- different contents with crossing versions.
select pg_advisory_xact_lock($1)
