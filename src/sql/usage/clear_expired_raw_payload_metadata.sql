-- Clear continuation metadata for expired raw rows in one metadata chunk.
--
-- This runs inside the same per-chunk transaction as the raw chunk drop.
-- `$1` is the exact prune cutoff, `$2`/`$3` are the paired metadata chunk
-- range. Only rows in that chunk are updated, so no other raw chunk or
-- metadata chunk is read or written by the full-drop branch.
--
--   $1  exact prune cutoff
--   $2  paired metadata chunk range start
--   $3  paired metadata chunk range end
UPDATE request_records rr
SET
    request_has_previous_response_id = FALSE,
    request_previous_response_id = NULL,
    request_previous_response_parent_found = NULL,
    request_conversation_key = NULL,
    request_conversation_parent_found = NULL
FROM request_record_raw_payloads raw
WHERE raw.created_at < $1
  AND rr.event_id = raw.event_id
  AND rr.created_at >= $2
  AND rr.created_at < $3;
