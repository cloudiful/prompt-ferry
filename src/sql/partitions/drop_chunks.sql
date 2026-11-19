-- Issue #69 Phase P4b: drop the expired chunks of one request-family parent.
--
-- The parent is always one of the eight compile-time names in
-- `partition_maintenance`, bound as text and resolved through `regclass`.
-- The cutoff is a whole UTC day at or before yesterday's midnight, so the
-- current day chunk is never selected. The count is the number of whole
-- chunks TimescaleDB actually dropped.
SELECT COUNT(*)::BIGINT AS "dropped!"
FROM drop_chunks($1::text::regclass, older_than => $2::timestamptz);
