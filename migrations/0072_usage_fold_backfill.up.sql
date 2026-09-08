-- P3 (issue #194): backfill the historical `input_tokens` that were stored in
-- the Anthropic "folded" shape, where the cache meters are already counted
-- inside `input_tokens`. Leaving them folded makes the ordinary-input
-- computation (`input_tokens - cache_read_tokens - cache_write_tokens`)
-- double-subtract the cache over history, so the cache fraction ends up billed
-- at `input_rate` on top of the discounted `cache_read_rate`/`cache_write_rate`
-- rows, overcharging `cache_total * input_rate`.
--
-- We only touch rows whose stored input is provably folded: a well-formed
-- non-folded row satisfies `input_tokens + output_tokens == total_tokens`, i.e.
-- `input_tokens == total_tokens - output_tokens`. Requiring
-- `input_tokens > total_tokens - output_tokens` therefore selects only the
-- inflated rows and never a normal ordinary row ("防碰 ordinary 行"). The cache
-- meter is read as `COALESCE(cache_read_tokens, cached_tokens, 0)` to honour the
-- legacy `cached_tokens` column used by older records, matching the bucket SQL.
--
-- NOTE (issue #200 P2): the strict `>` above misses the equality shape
-- `input_tokens == total_tokens - output_tokens` with `cache_read > 0`
-- (P1 observed 342410 folded rows in this shape). Those rows stay folded until
-- 0073 rewrites them with `>=`; the `cache > 0` and `total >= output` guards
-- are preserved there, and already-backfilled ordinary rows stay untouched
-- because `ordinary == total - output - cache < total - output` fails `>=`.
--
-- Guard: `total_tokens >= output_tokens` is required because when
-- `total_tokens < output_tokens` the difference `total_tokens - output_tokens`
-- is negative, which would make `input_tokens > (total_tokens - output_tokens)`
-- true for every non-negative `input_tokens` and misclassify a non-folded row as
-- folded. Excluding that malformed shape keeps the rewrite to genuinely folded
-- rows only.
--
-- The on-disk rewrite of `input_tokens` does not change the billing-table split
-- itself: `usage_charges.input_tokens` (and its `usage_charge_lines`) were
-- already computed as the ordinary input via `NormalizedBillingUsage`, i.e.
-- `greatest(input - cache_read - cache_write, 0)`. After this backfill the
-- existing rows stay consistent; only the raw `request_records.input_tokens`
-- now matches that ordinary value. Post-0072 the column is already ordinary, so
-- no consumer of `request_records.input_tokens` may re-apply
-- `greatest(input_tokens - cache_read_tokens - cache_write_tokens, 0)`; doing so
-- would double-subtract the already-backfilled values. Closed-loop check:
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
  AND COALESCE(input_tokens, 0) > COALESCE(total_tokens, 0) - COALESCE(output_tokens, 0);
