SELECT
    o.conversation_id AS "conversation_id!",
    o.endpoint_id,
    o.endpoint_key_id,
    (SELECT k.key_label FROM endpoint_api_keys k WHERE k.key_id = o.endpoint_key_id) AS endpoint_key_label,
    (SELECT pe.name FROM provider_endpoints pe WHERE pe.endpoint_id = o.endpoint_id) AS endpoint_name,
    o.created_by_user_id,
    o.created_at AS "created_at!",
    o.updated_at
FROM conversation_endpoint_overrides o
WHERE o.conversation_id = $1
