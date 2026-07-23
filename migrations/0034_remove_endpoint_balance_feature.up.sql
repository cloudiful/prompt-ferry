UPDATE request_records
SET route_selection_reason = 'default'
WHERE route_selection_reason = 'balance_weighted';

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_route_selection_reason;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_route_selection_reason
CHECK (
    route_selection_reason IN (
        'default',
        'session_affinity',
        'conversation_override',
        'avoidance_fallback'
    )
);

ALTER TABLE request_records
DROP COLUMN IF EXISTS route_balance_share,
DROP COLUMN IF EXISTS route_balance_unit,
DROP COLUMN IF EXISTS route_balance_value;

DROP TABLE IF EXISTS endpoint_balance_snapshots;

ALTER TABLE provider_endpoints
DROP COLUMN IF EXISTS balance_api_key,
DROP COLUMN IF EXISTS balance_api_user;
