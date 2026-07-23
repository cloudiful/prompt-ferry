DELETE FROM request_records
WHERE event_kind = 'upstream_attempt';

DROP INDEX IF EXISTS idx_request_records_request_kind_created_at;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_usage_events_event_kind;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_event_kind;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_event_kind
CHECK (event_kind = 'request');

UPDATE request_records
SET route_selection_reason = 'default'
WHERE route_selection_reason = 'avoidance_fallback';

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

DROP TABLE IF EXISTS model_avoidances;
