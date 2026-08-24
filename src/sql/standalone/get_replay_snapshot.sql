-- Fetch the latest standalone replay snapshot for a conversation.
-- Returns NULL when no snapshot has been persisted for the conversation.
-- The hydration path on standalone startup uses this to restore the
-- most recent checkpoint so runtime replay consumers see the same
-- prompt-ref set as the previous process instance.
SELECT
    conversation_id,
    base_event_id,
    conversation_seq,
    prompt_refs_json,
    ref_count,
    byte_size,
    updated_at
FROM standalone_replay_snapshots
WHERE conversation_id = ?;
