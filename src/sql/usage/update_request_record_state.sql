UPDATE request_records
SET request_state = $1,
    endpoint_id = COALESCE($2, endpoint_id),
    model_route_rule_id = COALESCE($3, model_route_rule_id),
    model = COALESCE($4, model),
    endpoint_key_id = COALESCE($5, endpoint_key_id),
    endpoint_key_label = COALESCE($6, endpoint_key_label),
    updated_at = NOW()
WHERE request_id = $7
  AND event_kind = 'request'
