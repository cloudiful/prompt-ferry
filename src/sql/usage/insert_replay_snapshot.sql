INSERT INTO request_record_replay_snapshots(
    created_at,
    event_id,
    conversation_id,
    conversation_seq,
    base_event_id,
    prompt_refs_json,
    ref_count,
    byte_size
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (event_id, created_at) DO NOTHING
