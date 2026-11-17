-- Allocate the next global relay snapshot fence on the publication connection.
--
-- The sequence is non-transactional: the token survives a rollback, is never
-- reused, and cannot be minted on a second connection after the publication
-- boundary was lost.
select nextval('relay_snapshot_version_seq') as version
