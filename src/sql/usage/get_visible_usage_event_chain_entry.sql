SELECT rr.event_id AS "event_id!", rr.request_id AS "request_id!", rr.user_id, rr.endpoint_id, rr.path AS "path!", rr.model, rr.conversation_id, rr.parent_event_id,
       rr.conversation_seq, rr.conversation_source AS "conversation_source!", rr.client_installation_id, rr.normalized_item_count,
       rr.normalized_chain_hash, rr.normalized_first_ref_hash, rr.normalized_last_ref_hash,
       rr.request_storage_mode AS "request_storage_mode!", rr.request_full_json, rr.request_delta_json,
       CAST(NULL AS JSONB) AS request_raw_json, rr.request_has_previous_response_id AS "request_has_previous_response_id!",
       rr.request_previous_response_id, rr.request_previous_response_parent_found,
       rr.request_conversation_key, rr.request_conversation_parent_found,
       rr.provider_response_id, rr.base_checkpoint_event_id, rr.response_prompt, CAST(NULL AS TEXT) AS response_raw_body
FROM request_records rr
WHERE rr.event_id = $1
  AND ($2::BIGINT IS NULL OR rr.user_id = $2)
