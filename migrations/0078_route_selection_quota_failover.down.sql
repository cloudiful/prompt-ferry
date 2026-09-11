UPDATE request_records
SET route_selection_reason = 'default'
WHERE route_selection_reason = 'quota_failover';

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
