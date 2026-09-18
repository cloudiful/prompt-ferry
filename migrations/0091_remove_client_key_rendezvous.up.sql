-- Issue #466: remove client_key_rendezvous, unify multi-key routing on responses_session_affinity.
UPDATE model_endpoint_rules
SET routing_strategy = 'responses_session_affinity'
WHERE routing_strategy = 'client_key_rendezvous';

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_routing_strategy;

ALTER TABLE model_endpoint_rules
ADD CONSTRAINT ck_model_endpoint_rules_routing_strategy
CHECK (routing_strategy IN ('responses_session_affinity'));
