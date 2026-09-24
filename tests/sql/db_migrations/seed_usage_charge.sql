INSERT INTO usage_charges (
    event_id,
    user_id,
    request_id,
    usage_status,
    pricing_status
)
VALUES ($1, $2, $3, 'known', 'unpriced')
