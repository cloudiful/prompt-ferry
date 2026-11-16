-- Serialize worker registry registrations.
--
-- The lock is transaction scoped, so a crashed registrar never leaves the
-- registry locked and two registrars never decide concurrently who owns an
-- identity.
select pg_advisory_xact_lock($1)
