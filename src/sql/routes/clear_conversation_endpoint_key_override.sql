UPDATE conversation_endpoint_overrides
SET endpoint_key_id = NULL,
    updated_at = NOW()
WHERE conversation_id = $1
  AND endpoint_key_id IS NOT NULL
