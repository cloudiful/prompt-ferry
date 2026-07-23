UPDATE request_records
SET route_selection_reason = 'session_affinity'
WHERE route_selection_reason = 'session_load_balance';

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_route_selection_reason;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_route_selection_reason
CHECK (
    route_selection_reason IN (
        'default',
        'session_affinity',
        'conversation_override'
    )
);

ALTER TABLE model_endpoint_rules
DROP CONSTRAINT IF EXISTS ck_model_endpoint_rules_session_affinity_lock_after_turns;

ALTER TABLE model_endpoint_rules
DROP COLUMN IF EXISTS session_affinity_lock_after_turns;
