DELETE FROM model_endpoint_rules r
WHERE NOT EXISTS (
    SELECT 1
    FROM model_route_targets t
    WHERE t.rule_id = r.rule_id
)
