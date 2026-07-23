INSERT INTO usage_charges (
    event_id, request_id, user_id, client_key_id, client_key_label,
    requested_model, upstream_model, endpoint_id, endpoint_key_id,
    usage_status, pricing_status, currency, provider_cost, customer_amount, adjusted_amount
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'CNY', $12, $13, $14)
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
    pricing_status = CASE
        WHEN EXISTS (SELECT 1 FROM usage_charge_adjustments WHERE charge_id = usage_charges.charge_id)
            THEN 'adjusted'
        ELSE EXCLUDED.pricing_status
    END,
    provider_cost = EXCLUDED.provider_cost,
    customer_amount = EXCLUDED.customer_amount,
    adjusted_amount = CASE
        WHEN EXISTS (SELECT 1 FROM usage_charge_adjustments WHERE charge_id = usage_charges.charge_id)
            THEN usage_charges.adjusted_amount
        ELSE EXCLUDED.adjusted_amount
    END,
    updated_at = NOW()
RETURNING charge_id
