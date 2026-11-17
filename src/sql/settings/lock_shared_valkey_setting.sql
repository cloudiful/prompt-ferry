-- Serialize shared setting writes that must not interleave.
--
-- The lock is transaction scoped: it is released when the surrounding
-- transaction commits or rolls back, so a crashed writer never leaves the
-- setting locked.
select pg_advisory_xact_lock($1)
