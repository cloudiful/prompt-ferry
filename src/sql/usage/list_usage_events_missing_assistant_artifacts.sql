SELECT ue.event_id, ue.request_id, ue.user_id, ue.endpoint_id, ue.path, ue.model, ue.conversation_id,
       ue.parent_event_id, ue.conversation_seq, ue.conversation_source, ue.client_installation_id,
       ue.normalized_item_count, ue.normalized_chain_hash, ue.normalized_first_ref_hash,
       ue.normalized_last_ref_hash, ue.request_storage_mode, ue.request_full_json,
       ue.request_delta_json, raw.request_raw_json,
       ue.request_has_previous_response_id, ue.request_previous_response_id,
       ue.request_previous_response_parent_found, ue.request_conversation_key,
       ue.request_conversation_parent_found, ue.provider_response_id,
       ue.base_checkpoint_event_id, ue.response_prompt, raw.response_raw_body
FROM request_records ue
LEFT JOIN request_record_assistant_artifacts ua ON ua.event_id = ue.event_id
LEFT JOIN request_record_raw_payloads raw
  ON raw.event_id = ue.event_id
 AND raw.created_at = ue.created_at
WHERE ua.event_id IS NULL
  AND ue.request_state = 'completed'
  AND ue.path IN ('/v1/responses', '/v1/chat/completions')
ORDER BY ue.event_id ASC
