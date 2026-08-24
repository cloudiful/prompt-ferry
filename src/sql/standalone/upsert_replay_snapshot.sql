-- Atomic monotonic upsert for the latest standalone replay snapshot per
-- conversation. The `WHERE` clause gates the `DO UPDATE` branch so an
-- incoming snapshot that would regress the stored checkpoint is silently
-- dropped: the row is preserved as-is and zero rows are affected, which
-- lets the store distinguish an accepted write from a rejected one
-- without a separate read-then-compare transaction.
--
-- Acceptance rules (matches the prompt refs ordering in
-- `replay_cache::newer_snapshot_wins_by_seq_then_event_id`):
--
--   1. Higher incoming `conversation_seq` always wins.
--   2. Equal `conversation_seq`: higher incoming `base_event_id` wins.
--      This avoids losing a fresher insert that happens to share the
--      same sequence number.
--   3. Lower incoming sequence, or equal sequence with lower base
--      event id, leaves the existing row untouched (no-op).
--
-- Returns the number of rows affected so the storage layer can surface
-- whether the upsert was actually applied.

INSERT INTO standalone_replay_snapshots(
    conversation_id,
    base_event_id,
    conversation_seq,
    prompt_refs_json,
    ref_count,
    byte_size,
    updated_at
)
VALUES (?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(conversation_id) DO UPDATE SET
    base_event_id = excluded.base_event_id,
    conversation_seq = excluded.conversation_seq,
    prompt_refs_json = excluded.prompt_refs_json,
    ref_count = excluded.ref_count,
    byte_size = excluded.byte_size,
    updated_at = excluded.updated_at
WHERE excluded.conversation_seq > standalone_replay_snapshots.conversation_seq
   OR (
       excluded.conversation_seq = standalone_replay_snapshots.conversation_seq
       AND excluded.base_event_id > standalone_replay_snapshots.base_event_id
   );
