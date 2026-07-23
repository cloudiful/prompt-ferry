ALTER TABLE model_endpoint_rules
ADD COLUMN IF NOT EXISTS session_affinity_lock_after_turns INTEGER NOT NULL DEFAULT 5;

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_session_affinity_lock_after_turns;

ALTER TABLE model_endpoint_rules
ADD CONSTRAINT ck_model_endpoint_rules_session_affinity_lock_after_turns
CHECK (session_affinity_lock_after_turns > 0);

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_route_selection_reason;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_route_selection_reason
CHECK (
    route_selection_reason IN (
        'default',
        'session_affinity',
        'session_load_balance',
        'conversation_override'
    )
);
