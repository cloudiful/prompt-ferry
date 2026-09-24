SELECT COUNT(*)::BIGINT AS "count!"
FROM request_record_replay_snapshots
WHERE event_id = $1
   OR base_event_id = $1;
