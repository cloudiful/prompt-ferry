-- Database-side whole-minute cap for the rollup refresh window. The caller
-- computes floor/ceiling bounds; this returns the current minute so a future
-- or incomplete bucket is never materialized.
SELECT date_trunc('minute', NOW()) AS "current_minute!"
