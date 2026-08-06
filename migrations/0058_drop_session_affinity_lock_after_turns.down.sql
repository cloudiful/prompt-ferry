ALTER TABLE model_endpoint_rules
ADD COLUMN IF NOT EXISTS session_affinity_lock_after_turns INTEGER NOT NULL DEFAULT 5;

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_session_affinity_lock_after_turns;

ALTER TABLE model_endpoint_rules
ADD CONSTRAINT ck_model_endpoint_rules_session_affinity_lock_after_turns
CHECK (session_affinity_lock_after_turns > 0);
