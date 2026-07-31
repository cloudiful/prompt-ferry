ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS abort_reason TEXT,
ADD COLUMN IF NOT EXISTS abort_from_state TEXT,
ADD COLUMN IF NOT EXISTS abort_response_started BOOLEAN;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_abort_reason;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_abort_reason
CHECK (
    abort_reason IS NULL
    OR abort_reason IN (
        'downstream_closed',
        'bridge_backpressure_full',
        'bridge_backpressure_bytes_limit',
        'worker_lease_expired',
        'valkey_lease_missing',
        'relay_unknown'
    )
);

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_abort_from_state;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_abort_from_state
CHECK (
    abort_from_state IS NULL
    OR abort_from_state IN ('received', 'awaiting_approval', 'upstream_processing')
);
