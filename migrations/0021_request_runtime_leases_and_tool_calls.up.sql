ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS owner_worker_id UUID;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS last_heartbeat_at TIMESTAMPTZ;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_request_state;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_request_state
CHECK (request_state IN (
    'received',
    'awaiting_approval',
    'upstream_processing',
    'completed',
    'failed',
    'aborted'
));

CREATE INDEX IF NOT EXISTS idx_request_records_active_lease
ON request_records(lease_expires_at ASC, updated_at ASC)
WHERE event_kind = 'request'
  AND request_state IN ('received', 'awaiting_approval', 'upstream_processing');

CREATE TABLE IF NOT EXISTS conversation_counters (
    conversation_id UUID PRIMARY KEY,
    next_seq INTEGER NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS request_record_tool_calls (
    tool_call_event_id BIGSERIAL PRIMARY KEY,
    parent_event_id BIGINT NOT NULL REFERENCES request_records(event_id) ON DELETE CASCADE,
    conversation_id UUID,
    call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_json JSONB,
    arguments_preview TEXT,
    status TEXT NOT NULL DEFAULT 'emitted',
    sequence_in_turn INTEGER,
    mcp_request_event_id BIGINT REFERENCES request_records(event_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_request_record_tool_calls_status
        CHECK (status IN ('emitted', 'output_received', 'failed', 'skipped')),
    CONSTRAINT uq_request_record_tool_calls_parent_call
        UNIQUE(parent_event_id, call_id)
);

CREATE INDEX IF NOT EXISTS idx_request_record_tool_calls_parent
ON request_record_tool_calls(parent_event_id, sequence_in_turn ASC, tool_call_event_id ASC);

CREATE INDEX IF NOT EXISTS idx_request_record_tool_calls_conversation_status
ON request_record_tool_calls(conversation_id, status, created_at ASC);
