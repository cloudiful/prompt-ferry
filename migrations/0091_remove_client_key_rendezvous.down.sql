-- Issue #466 down: restore the two-value CHECK; data is not written back.
ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_routing_strategy;

ALTER TABLE model_endpoint_rules
ADD CONSTRAINT ck_model_endpoint_rules_routing_strategy
CHECK (routing_strategy IN ('client_key_rendezvous', 'responses_session_affinity'));
