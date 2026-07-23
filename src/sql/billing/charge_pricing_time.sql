SELECT COALESCE(
    (SELECT created_at FROM usage_charges WHERE event_id = $1),
    (SELECT created_at FROM request_records WHERE event_id = $1),
    NOW()
) AS "created_at!"
