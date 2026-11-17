-- Try to enter the relay snapshot publication boundary without waiting.
--
-- The lock is transaction scoped, so the boundary is held only while the
-- snapshot contents are read and the version is assigned. A caller that does
-- not get the lock rolls its transaction back and retries later, so a waiting
-- publisher never holds a database connection while it waits.
select pg_try_advisory_xact_lock($1) as acquired
