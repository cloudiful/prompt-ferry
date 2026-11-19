-- Drop exactly one raw chunk by its catalog range.
--
-- The caller has already locked the chunk and verified its range. `drop_chunks`
-- is bounded on both sides, so only the listed chunk can be returned. The
-- caller requires exactly one returned regclass name and matches it to the
-- listed chunk before committing.
--
--   $1  raw parent regclass text
--   $2  chunk range end (exclusive newer_than bound)
--   $3  chunk range start (inclusive older_than bound)
SELECT dropped::text AS "dropped!"
FROM drop_chunks(
    $1::text::regclass,
    older_than => $2::timestamptz,
    newer_than => $3::timestamptz
) AS dropped;
