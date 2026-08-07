SELECT
    event_id AS "record_id!",
    user_id,
    conversation_id,
    model,
    model_route_rule_id
FROM request_records
WHERE event_id = $1
