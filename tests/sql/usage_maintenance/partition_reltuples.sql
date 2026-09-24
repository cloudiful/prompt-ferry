-- Issue #277 Phase P11: planner stats for one child partition.
-- `reltuples` is -1 on a never-analyzed partition and >= 0 (0 for empty)
-- once ANALYZE has collected statistics for it.
SELECT c.reltuples::BIGINT AS "reltuples!"
FROM pg_class c
JOIN pg_namespace ns ON ns.oid = c.relnamespace
WHERE c.relname = $1
  AND ns.nspname = current_schema();
