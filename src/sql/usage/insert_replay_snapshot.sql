INSERT INTO request_record_replay_snapshots(
    event_id,
    conversation_id,
    conversation_seq,
    base_event_id,
    prompt_refs_json,
    ref_count,
    byte_size
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
ON CONFLICT (event_id) DO NOTHING
