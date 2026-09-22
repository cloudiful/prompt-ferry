-- Issue #548 Task 1: cache-rate alert candidates per conversation.
--
-- One row per conversation whose completed AI turns inside the window reach
-- `min_turns`. The caller turns `cache_read`/`full_input` into a rate with the
-- shared `overview_cache_rate` semantics and applies the threshold, so this
-- query never decides the alert rate itself.
--
-- The denominator is the fold-aware `normalized_full_input_tokens` from
-- `overview/metrics.sql` (ordinary + read + write, with the 0072/0073
-- still-folded `max` fallback) summed row by row, so still-folded history is
-- not double-counted. Only `request_state = 'completed'` AI request rows are
-- counted; failed, aborted, and in-flight rows are ignored. Turns are counted
-- once per `conversation_seq` so a repeated lifecycle row for the same turn
-- cannot inflate the count, and `model` is the most recent turn's model for
-- the conversation (a mid-conversation model switch never splits one alert
-- into two).
WITH normalized AS (
    SELECT rr.event_id,
           rr.conversation_id,
           rr.conversation_seq,
           rr.model,
           rr.created_at,
           COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0)::BIGINT AS normalized_cache_read_tokens,
           CASE
               WHEN COALESCE(COALESCE(rr.cache_read_tokens, rr.cached_tokens), 0) > 0
                   AND COALESCE(rr.total_tokens, 0) >= COALESCE(rr.output_tokens, 0)
                   AND COALESCE(rr.input_tokens, 0)
                       >= COALESCE(rr.total_tokens, 0) - COALESCE(rr.output_tokens, 0)
               THEN GREATEST(
                   COALESCE(rr.input_tokens, 0),
                   GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                       + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0),
                   0
               )
               ELSE GREATEST(COALESCE(rr.input_tokens, 0), 0)
                   + GREATEST(COALESCE(rr.cache_read_tokens, rr.cached_tokens, 0), 0)
                   + GREATEST(COALESCE(rr.cache_write_tokens, 0), 0)
           END::BIGINT AS normalized_full_input_tokens
    FROM request_records rr
    WHERE rr.event_kind = 'request'
      AND rr.request_category = 'ai'
      AND rr.request_state = 'completed'
      AND rr.conversation_id IS NOT NULL
      AND rr.created_at >= NOW() - make_interval(mins => $1::INT)
), turns AS (
    SELECT DISTINCT ON (conversation_id, COALESCE(conversation_seq::BIGINT, -event_id))
           conversation_id,
           model,
           created_at,
           normalized_cache_read_tokens,
           normalized_full_input_tokens
    FROM normalized
    ORDER BY conversation_id,
             COALESCE(conversation_seq::BIGINT, -event_id),
             created_at DESC
)
SELECT conversation_id AS "conversation_id!",
       (ARRAY_AGG(model ORDER BY created_at DESC) FILTER (WHERE model IS NOT NULL))[1] AS model,
       COUNT(*)::INT AS "turns!",
       COALESCE(SUM(normalized_cache_read_tokens), 0)::BIGINT AS "cache_read!",
       COALESCE(SUM(normalized_full_input_tokens), 0)::BIGINT AS "full_input!",
       NOW() - make_interval(mins => $1::INT) AS "window_start!",
       NOW() AS "window_end!"
FROM turns
GROUP BY conversation_id
HAVING COUNT(*) >= $2::INT
