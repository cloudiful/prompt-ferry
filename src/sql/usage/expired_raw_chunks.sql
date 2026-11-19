-- Raw parent chunks that still hold rows older than the exact prune cutoff.
--
-- The cutoff is `now - raw_retention_days`. A chunk is listed only when it
-- starts before the cutoff and an actual raw row inside its catalog range is
-- older than the cutoff, so empty and fully recent chunks are never locked.
-- The server formats the ACCESS EXCLUSIVE statement; the caller takes that
-- lock before looking at compression state or deleting anything.
--
--   $1  exact prune cutoff
SELECT
    ch.chunk_schema AS "chunk_schema!",
    ch.chunk_name AS "chunk_name!",
    ch.range_start AS "range_start!",
    ch.range_end AS "range_end!",
    format(
        'LOCK TABLE %I.%I IN ACCESS EXCLUSIVE MODE',
        ch.chunk_schema,
        ch.chunk_name
    ) AS "lock_sql!"
FROM timescaledb_information.chunks ch
WHERE ch.hypertable_schema = 'public'
  AND ch.hypertable_name = 'request_record_raw_payloads'
  AND ch.range_start < $1
  AND EXISTS (
      SELECT 1
      FROM public.request_record_raw_payloads raw
      WHERE raw.created_at >= ch.range_start
        AND raw.created_at < ch.range_end
        AND raw.created_at < $1
  )
ORDER BY ch.range_start, ch.chunk_schema, ch.chunk_name;
