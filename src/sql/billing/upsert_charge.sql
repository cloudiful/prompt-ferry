INSERT INTO usage_charges (
    event_id, request_id, user_id, client_key_id, client_key_label,
    requested_model, upstream_model, endpoint_id, endpoint_key_id,
    usage_status, pricing_status, currency, customer_amount,
    input_tokens, cache_read_tokens, cache_write_tokens, output_tokens
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'CNY', $12, $13, $14, $15, $16)
ON CONFLICT (event_id)
DO UPDATE SET
    event_id = EXCLUDED.event_id,
    request_id = EXCLUDED.request_id,
    user_id = COALESCE(EXCLUDED.user_id, usage_charges.user_id),
    client_key_id = COALESCE(EXCLUDED.client_key_id, usage_charges.client_key_id),
    client_key_label = COALESCE(EXCLUDED.client_key_label, usage_charges.client_key_label),
    requested_model = COALESCE(EXCLUDED.requested_model, usage_charges.requested_model),
    upstream_model = COALESCE(EXCLUDED.upstream_model, usage_charges.upstream_model),
    endpoint_id = COALESCE(EXCLUDED.endpoint_id, usage_charges.endpoint_id),
    endpoint_key_id = COALESCE(EXCLUDED.endpoint_key_id, usage_charges.endpoint_key_id),
    usage_status = EXCLUDED.usage_status,
    pricing_status = EXCLUDED.pricing_status,
    customer_amount = EXCLUDED.customer_amount,
    input_tokens = EXCLUDED.input_tokens,
    cache_read_tokens = EXCLUDED.cache_read_tokens,
    cache_write_tokens = EXCLUDED.cache_write_tokens,
    output_tokens = EXCLUDED.output_tokens,
    updated_at = NOW()
RETURNING charge_id AS "charge_id!"
