-- Chunks the clear will touch, with the server-quoted lock statement for each.
--
-- The seven deleted parents contribute the chunks whose range intersects the
-- inclusive candidate envelope. Prompt blocks contribute the chunks holding
-- the captured block hashes dated before the current UTC day. Uncompressed
-- chunks are listed too: the caller locks every touched chunk so the
-- compression policy cannot take one mid-clear; only compressed ones are
-- decompressed.
--
-- `format('%I.%I', ...)` runs on the server, so the lock statement never
-- depends on client-side identifier quoting. Compression state is deliberately
-- not projected here: it is re-read after all locks are held.
--
--   $1  envelope min created_at (inclusive)
--   $2  envelope max created_at (inclusive)
--   $3  captured candidate block hashes
SELECT
    ch.chunk_schema AS "chunk_schema!",
    ch.chunk_name AS "chunk_name!",
    format('LOCK TABLE %I.%I IN ROW SHARE MODE', ch.chunk_schema, ch.chunk_name)
        AS "lock_sql!"
FROM timescaledb_information.chunks ch
WHERE ch.hypertable_schema = 'public'
  AND (
      (
          ch.hypertable_name IN (
              'request_records',
              'request_record_content',
              'request_record_block_refs',
              'request_record_assistant_artifacts',
              'request_record_tool_calls',
              'request_record_replay_snapshots',
              'request_record_raw_payloads'
          )
          AND ch.range_end > $1
          AND ch.range_start <= $2
      )
      OR (
          ch.hypertable_name = 'usage_prompt_blocks'
          AND EXISTS (
              SELECT 1
              FROM usage_prompt_blocks upb
              WHERE upb.block_hash = ANY($3::TEXT[])
                AND upb.created_at < (
                    date_trunc('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'
                )
                AND upb.created_at >= ch.range_start
                AND upb.created_at < ch.range_end
          )
      )
  )
ORDER BY ch.chunk_schema, ch.chunk_name;
