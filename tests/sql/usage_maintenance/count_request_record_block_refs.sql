SELECT COUNT(*)::BIGINT AS "count!"
FROM request_record_block_refs
WHERE event_id = $1;
