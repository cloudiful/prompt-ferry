-- P2 (issue #205): 2026-09-08 single-day unfold + total closed loop.
--
-- Replaces the reverted P2-attempt-1 approach that appended this logic to the
-- already-deployed 0074 (reviewer 4866 P0: `sqlx::migrate!` marks 0074 as
-- applied via `_sqlx_migrations.version`, so appended statements never run on
-- dev/staging/prod). This is a fresh 0075 migration so it actually executes.
--
-- Scope is pinned to 2026-09-08 (149 still-folded rows) so historical rows
-- (~20M, already handled by 0072/0073) are never touched and no full-table
-- scan/rewrite occurs.
--
-- Step 1: unfold the remaining 9/8 folded inputs (same guard as 0073:
-- cache>0 AND total>=output AND input>=total-output). Already-ordinary rows
-- satisfy input==total-output-cache<total-output and are never rewritten.
UPDATE request_records
SET input_tokens = GREATEST(
    COALESCE(input_tokens, 0)
        - COALESCE(COALESCE(cache_read_tokens, cached_tokens), 0)
        - COALESCE(cache_write_tokens, 0),
    0
)
WHERE event_kind = 'request'
  AND created_at >= DATE '2026-09-08'
  AND created_at < DATE '2026-09-09'
  AND COALESCE(cache_read_tokens, cached_tokens, 0) > 0
  AND COALESCE(total_tokens, 0) >= COALESCE(output_tokens, 0)
  AND COALESCE(input_tokens, 0) >= COALESCE(total_tokens, 0) - COALESCE(output_tokens, 0);

-- Step 2: rewrite halved totals on 9/8 to the closed loop
-- ordinary+read+write+output. Rows already closed (ordinary+cache+output ==
-- total, including just-unfolded rows whose total already equals the sum)
-- fail the `!=` guard and are left untouched.
UPDATE request_records
SET total_tokens = GREATEST(COALESCE(input_tokens, 0), 0)
    + GREATEST(COALESCE(COALESCE(cache_read_tokens, cached_tokens), 0), 0)
    + GREATEST(COALESCE(cache_write_tokens, 0), 0)
    + GREATEST(COALESCE(output_tokens, 0), 0)
WHERE event_kind = 'request'
  AND created_at >= DATE '2026-09-08'
  AND created_at < DATE '2026-09-09'
  AND COALESCE(total_tokens, 0) != GREATEST(COALESCE(input_tokens, 0), 0)
    + GREATEST(COALESCE(COALESCE(cache_read_tokens, cached_tokens), 0), 0)
    + GREATEST(COALESCE(cache_write_tokens, 0), 0)
    + GREATEST(COALESCE(output_tokens, 0), 0);
