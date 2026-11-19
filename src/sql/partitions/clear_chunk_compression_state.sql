-- Current compression state of one listed chunk.
--
-- Read only after the clear has acquired its ROW SHARE lock on the chunk and
-- before deciding whether to decompress it. READ COMMITTED gives every
-- statement a fresh snapshot, so a compression committed while the clear was
-- waiting for the lock is visible here even though the chunk list saw the old
-- rowstore state. A missing row means the chunk disappeared mid-clear and the
-- caller must fail instead of assuming it is uncompressed.
--
--   $1  chunk schema
--   $2  chunk name
SELECT is_compressed AS "is_compressed!"
FROM timescaledb_information.chunks
WHERE chunk_schema = $1
  AND chunk_name = $2;
