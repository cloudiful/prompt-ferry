CREATE TABLE IF NOT EXISTS request_record_replay_snapshots (
    event_id BIGINT PRIMARY KEY REFERENCES request_records(event_id) ON DELETE CASCADE,
    conversation_id UUID NOT NULL,
    conversation_seq INTEGER NOT NULL,
    base_event_id BIGINT NOT NULL REFERENCES request_records(event_id) ON DELETE CASCADE,
    prompt_refs_json JSONB NOT NULL,
    ref_count INTEGER NOT NULL,
    byte_size INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_request_record_replay_snapshots_conversation_seq
ON request_record_replay_snapshots(conversation_id, conversation_seq DESC, event_id DESC);
