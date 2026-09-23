-- Issue #277 Phase P5: bound every maintenance batch.
--
-- A slow plan must not hold locks or a worker connection indefinitely, so each
-- batch transaction starts with a 2s lock timeout and a 30s statement timeout.
-- `SET LOCAL` keeps the bound scoped to the current transaction only. Executed
-- through the simple query protocol because PostgreSQL cannot prepare utility
-- statements.
SET LOCAL lock_timeout = '2s';
SET LOCAL statement_timeout = '30s';
