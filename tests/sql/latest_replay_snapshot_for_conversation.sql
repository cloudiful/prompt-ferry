SELECT conversation_seq,
       ref_count,
       byte_size
FROM request_record_replay_snapshots
WHERE conversation_id = $1
ORDER BY conversation_seq DESC, event_id DESC
LIMIT 1
