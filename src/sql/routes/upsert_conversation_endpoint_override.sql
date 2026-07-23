WITH upserted AS (
    INSERT INTO conversation_endpoint_overrides(
        conversation_id,
        endpoint_id,
        endpoint_key_id,
        created_by_user_id,
        created_at,
        updated_at
    )
    VALUES ($1, $2, $3, $4, NOW(), NOW())
    ON CONFLICT(conversation_id)
    DO UPDATE SET
        endpoint_id = EXCLUDED.endpoint_id,
        endpoint_key_id = EXCLUDED.endpoint_key_id,
        created_by_user_id = EXCLUDED.created_by_user_id,
        updated_at = NOW()
    RETURNING
        conversation_id,
        endpoint_id,
        endpoint_key_id,
        created_by_user_id,
        created_at,
        updated_at
)
SELECT
    upserted.conversation_id,
    upserted.endpoint_id,
    upserted.endpoint_key_id,
    k.key_label AS endpoint_key_label,
    pe.name AS endpoint_name,
    upserted.created_by_user_id,
    upserted.created_at,
    upserted.updated_at
FROM upserted
LEFT JOIN provider_endpoints pe ON pe.endpoint_id = upserted.endpoint_id
LEFT JOIN endpoint_api_keys k ON k.key_id = upserted.endpoint_key_id
