ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_routing_strategy;

ALTER TABLE model_endpoint_rules
DROP COLUMN IF EXISTS routing_strategy;
