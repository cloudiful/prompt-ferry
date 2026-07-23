ALTER TABLE model_endpoint_rules
ADD COLUMN IF NOT EXISTS routing_strategy TEXT NOT NULL DEFAULT 'client_key_rendezvous';

UPDATE model_endpoint_rules
SET routing_strategy = 'client_key_rendezvous'
WHERE routing_strategy IS NULL OR routing_strategy = '';

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_routing_strategy;

ALTER TABLE model_endpoint_rules
ADD CONSTRAINT ck_model_endpoint_rules_routing_strategy
CHECK (routing_strategy IN ('client_key_rendezvous', 'responses_session_affinity'));
