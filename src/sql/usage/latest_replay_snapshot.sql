SELECT event_id, conversation_id, conversation_seq, base_event_id, prompt_refs_json,
       ref_count, byte_size, created_at
FROM request_record_replay_snapshots
WHERE conversation_id = $1
ORDER BY conversation_seq DESC, event_id DESC
LIMIT 1
