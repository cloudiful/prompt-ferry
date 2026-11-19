-- Metadata chunk paired with one raw chunk by exact UTC day range.
--
-- Metadata retention may be shorter than raw retention, so an absent row is
-- a legitimate raw-only cleanup. A present row is locked by the caller before
-- its compression state is re-read and before the continuation metadata is
-- cleared.
--
--   $1  chunk range start
--   $2  chunk range end
SELECT
    ch.chunk_schema AS "chunk_schema!",
    ch.chunk_name AS "chunk_name!",
    format(
        'LOCK TABLE %I.%I IN ACCESS EXCLUSIVE MODE',
        ch.chunk_schema,
        ch.chunk_name
    ) AS "lock_sql!"
FROM timescaledb_information.chunks ch
WHERE ch.hypertable_schema = 'public'
  AND ch.hypertable_name = 'request_records'
  AND ch.range_start = $1
  AND ch.range_end = $2
ORDER BY ch.chunk_schema, ch.chunk_name;
