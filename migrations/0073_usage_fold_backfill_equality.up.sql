-- P2 (issue #200): backfill the equality-shape folded rows missed by 0072.
--
-- 0072 used strict `input_tokens > total_tokens - output_tokens` ("防碰 ordinary
-- 行") and therefore left rows with `input_tokens == total_tokens - output_tokens`
-- and `cache_read > 0` still folded (P1 observed 342410 rows). A folded row in
-- this shape satisfies `input_folded == total - output` because the stored input
-- already contains the cache (`ordinary + cache == total - output`), while a
-- genuine ordinary row satisfies `input == total - output - cache`, i.e.
-- `input < total - output` whenever `cache > 0`. Requiring `>=` therefore
-- selects exactly the remaining folded rows and never an ordinary row.
--
-- Guards preserved from 0072: `cache_read (or legacy cached) > 0` and
-- `total_tokens >= output_tokens` (excludes the malformed `total < output`
-- shape where the difference is negative and every non-negative input would
-- match). Already-backfilled ordinary rows have
-- `input == total - output - cache < total - output`, so `>=` is false and they
-- are never rewritten twice. Closed loop after rewrite:
-- ordinary (`input_tokens`) + `cache_read` + `cache_write` + `output` == `total`.
UPDATE request_records
SET input_tokens = GREATEST(
    COALESCE(input_tokens, 0)
        - COALESCE(COALESCE(cache_read_tokens, cached_tokens), 0)
        - COALESCE(cache_write_tokens, 0),
    0
)
WHERE event_kind = 'request'
  AND COALESCE(cache_read_tokens, cached_tokens, 0) > 0
  AND COALESCE(total_tokens, 0) >= COALESCE(output_tokens, 0)
  AND COALESCE(input_tokens, 0) >= COALESCE(total_tokens, 0) - COALESCE(output_tokens, 0);
