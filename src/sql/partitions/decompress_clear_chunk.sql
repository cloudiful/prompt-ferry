-- Decompress one touched chunk inside the clear transaction.
--
-- `if_compressed => true` makes the call idempotent: a chunk that another
-- process already decompressed between listing and this call stays valid, and
-- an uncompressed chunk is a no-op. Identifier quoting is done by the server.
--
--   $1  chunk schema
--   $2  chunk name
SELECT public.decompress_chunk(
    format('%I.%I', $1::TEXT, $2::TEXT)::regclass,
    if_compressed => true
)::TEXT AS "result";
