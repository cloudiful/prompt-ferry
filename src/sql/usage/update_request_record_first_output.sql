-- Issue #657 Phase P1: persist the first meaningful output instant on the
-- running request row so the request list can tell "waiting for response"
-- apart from "streaming output" without a new column or a new state.
-- Idempotent by construction: a value already recorded for this request always
-- wins, so a retry, a repeated observation, or a write that races the terminal
-- record can never move the instant or overwrite the final timing. Only
-- `ttft_ms` and `updated_at` are touched: the request state and every other
-- terminal field stay exactly as the request family wrote them.
UPDATE request_records
SET ttft_ms = COALESCE(ttft_ms, $1),
    updated_at = NOW()
WHERE request_id = $2
  AND event_kind = 'request'
  AND ($3::TIMESTAMPTZ IS NULL OR created_at = $3)
