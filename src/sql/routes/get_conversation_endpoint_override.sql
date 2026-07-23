SELECT
    o.conversation_id,
    o.endpoint_id,
    o.endpoint_key_id,
    k.key_label AS endpoint_key_label,
    pe.name AS endpoint_name,
    o.created_by_user_id,
    o.created_at,
    o.updated_at
FROM conversation_endpoint_overrides o
LEFT JOIN provider_endpoints pe ON pe.endpoint_id = o.endpoint_id
LEFT JOIN endpoint_api_keys k ON k.key_id = o.endpoint_key_id
WHERE o.conversation_id = $1
