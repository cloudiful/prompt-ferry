-- Issue #277 Phase P10 down fix — the down path must stay executable under
-- normal operation. The request family is dropped without being refilled, so
-- every surviving table that carries a foreign key into `request_records`
-- has to lose its carrying rows first: the billing ledger, the redaction
-- sessions and both raw payload carriers (data loss in both directions is
-- operator-approved). The original FK shape is then restored with no drift.

-- 1. The sequences are owned by the partitioned tables; detach them first.
ALTER SEQUENCE usage_events_event_id_seq OWNED BY NONE;
ALTER SEQUENCE request_record_tool_calls_tool_call_event_id_seq OWNED BY NONE;

-- 2. Drop the partitioned family (partitions go with the parent) so every
--    legacy table and index name is free again. No data is carried over.
DROP TABLE request_records;
DROP TABLE request_record_content;
DROP TABLE request_record_block_refs;
DROP TABLE usage_prompt_blocks;
DROP TABLE request_record_assistant_artifacts;
DROP TABLE request_record_tool_calls;
DROP TABLE request_record_replay_snapshots;

-- 3. Empty legacy tables with their original columns, keys, indexes and FKs.
CREATE TABLE request_records (
    event_id BIGINT NOT NULL DEFAULT nextval('usage_events_event_id_seq'),
    request_id UUID NOT NULL,
    user_id BIGINT,
    client_key_label TEXT,
    endpoint_id UUID,
    path TEXT NOT NULL,
    model TEXT,
    status INTEGER,
    ok BOOLEAN,
    duration_ms BIGINT,
    ttft_ms BIGINT,
    input_tokens BIGINT,
    output_tokens BIGINT,
    total_tokens BIGINT,
    cached_tokens BIGINT,
    cache_read_tokens BIGINT,
    cache_write_tokens BIGINT,
    conversation_id UUID,
    parent_event_id BIGINT,
    conversation_seq INTEGER,
    request_storage_mode TEXT NOT NULL DEFAULT 'full',
    request_full_json JSONB,
    request_delta_json JSONB,
    provider_response_id TEXT,
    base_checkpoint_event_id BIGINT,
    response_prompt TEXT,
    upstream_error_body TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    request_user_agent TEXT,
    request_has_previous_response_id BOOLEAN NOT NULL DEFAULT FALSE,
    request_previous_response_id TEXT,
    request_previous_response_parent_found BOOLEAN,
    client_installation_id TEXT,
    normalized_item_count INTEGER,
    normalized_chain_hash TEXT,
    normalized_first_ref_hash TEXT,
    normalized_last_ref_hash TEXT,
    conversation_source TEXT NOT NULL DEFAULT 'none',
    event_kind TEXT NOT NULL DEFAULT 'request',
    request_state TEXT NOT NULL DEFAULT 'completed',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    storage_sanitized BOOLEAN NOT NULL DEFAULT FALSE,
    storage_sanitized_nul_count INTEGER NOT NULL DEFAULT 0,
    request_category TEXT NOT NULL DEFAULT 'ai',
    model_route_rule_id UUID,
    mcp_server_id UUID,
    mcp_server_name TEXT,
    mcp_protocol_method TEXT,
    mcp_operation_name TEXT,
    failure_family TEXT,
    mcp_bearer_token_slot SMALLINT,
    route_selection_reason TEXT NOT NULL DEFAULT 'default',
    owner_worker_id UUID,
    lease_expires_at TIMESTAMPTZ,
    last_heartbeat_at TIMESTAMPTZ,
    http_request_content_encoding TEXT,
    http_request_compressed BOOLEAN NOT NULL DEFAULT FALSE,
    http_request_compressed_bytes BIGINT,
    http_request_decompressed_bytes BIGINT,
    http_request_compression_ratio DOUBLE PRECISION,
    provider_conversation_key TEXT,
    request_conversation_parent_found BOOLEAN,
    request_conversation_key TEXT,
    redaction_applied BOOLEAN NOT NULL DEFAULT FALSE,
    redaction_findings_count INTEGER NOT NULL DEFAULT 0,
    redaction_replacements_count INTEGER NOT NULL DEFAULT 0,
    redaction_types_json JSONB,
    redaction_fields_json JSONB,
    upstream_redaction_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    endpoint_key_id UUID,
    endpoint_key_label TEXT,
    response_capture_truncated BOOLEAN NOT NULL DEFAULT FALSE,
    client_key_id BIGINT,
    requested_model TEXT,
    upstream_model TEXT,
    content_expired_at TIMESTAMPTZ,
    abort_reason TEXT,
    abort_from_state TEXT,
    abort_response_started BOOLEAN,
    applied_thinking_effort_override TEXT
);

ALTER TABLE request_records
    ADD CONSTRAINT usage_events_pkey PRIMARY KEY (event_id);
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_event_kind CHECK (event_kind = 'request');
ALTER TABLE request_records
    ADD CONSTRAINT ck_usage_event_request_storage_mode CHECK (
        request_storage_mode IN ('full', 'append_delta')
    );
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_request_state CHECK (
        request_state IN ('received', 'awaiting_approval', 'upstream_processing', 'completed', 'failed', 'aborted')
    );
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_request_category CHECK (request_category IN ('ai', 'mcp'));
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_failure_family CHECK (
        failure_family IS NULL OR failure_family IN (
            'auth', 'rate_limit', 'quota', 'timeout', 'upstream_4xx',
            'upstream_5xx', 'network', 'empty_success', 'policy', 'unknown'
        )
    );
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_route_selection_reason CHECK (
        route_selection_reason IN (
            'default', 'session_affinity', 'session_load_balance', 'conversation_override', 'quota_failover'
        )
    );
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_mcp_bearer_token_slot CHECK (
        mcp_bearer_token_slot IS NULL OR mcp_bearer_token_slot > 0
    );
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_abort_reason CHECK (
        abort_reason IS NULL OR abort_reason IN (
            'downstream_closed', 'bridge_backpressure_full', 'bridge_backpressure_bytes_limit',
            'worker_lease_expired', 'valkey_lease_missing', 'relay_unknown'
        )
    );
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_abort_from_state CHECK (
        abort_from_state IS NULL OR abort_from_state IN (
            'received', 'awaiting_approval', 'upstream_processing'
        )
    );
ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_applied_thinking_effort CHECK (
        applied_thinking_effort_override IS NULL OR applied_thinking_effort_override IN (
            'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'
        )
    );

CREATE UNIQUE INDEX uq_request_records_request_id_request
    ON request_records (request_id)
    WHERE event_kind = 'request';
CREATE INDEX idx_request_records_category_created_at
    ON request_records (request_category, created_at DESC);
CREATE INDEX idx_request_records_client_key_created_at
    ON request_records (client_key_id, created_at DESC);
CREATE INDEX idx_request_records_content_expired_at
    ON request_records (content_expired_at, created_at, event_id)
    WHERE content_expired_at IS NULL;
CREATE INDEX idx_request_records_conversation_seq
    ON request_records (conversation_id, conversation_seq DESC);
CREATE INDEX idx_request_records_created_at
    ON request_records (created_at DESC);
CREATE INDEX idx_request_records_created_at_record_id
    ON request_records (created_at DESC, event_id DESC);
CREATE INDEX idx_request_records_endpoint_created_at
    ON request_records (endpoint_id, created_at DESC);
CREATE INDEX idx_request_records_failure_family_created_at
    ON request_records (failure_family, created_at DESC);
CREATE INDEX idx_request_records_inflight_request_id
    ON request_records (request_id)
    WHERE event_kind = 'request'
      AND request_state IN ('received', 'awaiting_approval', 'upstream_processing');
CREATE INDEX idx_request_records_mcp_server_created_at
    ON request_records (mcp_server_id, created_at DESC);
CREATE INDEX idx_request_records_model_created_at
    ON request_records (model, created_at DESC);
CREATE INDEX idx_request_records_model_route_rule_created_at
    ON request_records (model_route_rule_id, created_at DESC);
CREATE INDEX idx_request_records_provider_conversation_key
    ON request_records (provider_conversation_key, user_id, event_id DESC);
CREATE INDEX idx_request_records_provider_response_id
    ON request_records (provider_response_id);
CREATE INDEX idx_request_records_redaction_applied_created_at
    ON request_records (redaction_applied, created_at DESC);
CREATE INDEX idx_request_records_request_id
    ON request_records (request_id);
CREATE INDEX idx_request_records_requested_model_created_at
    ON request_records (requested_model, created_at DESC);
CREATE INDEX idx_request_records_token_slot_created_at
    ON request_records (mcp_bearer_token_slot, created_at DESC);
CREATE INDEX idx_request_records_usage_covering
    ON request_records (request_category, created_at DESC)
    INCLUDE (
        user_id, ok, request_state, duration_ms, ttft_ms, input_tokens, output_tokens,
        total_tokens, cached_tokens, cache_read_tokens, cache_write_tokens, mcp_protocol_method,
        model, endpoint_id, failure_family, mcp_server_id, mcp_server_name, client_key_id,
        client_key_label
    )
    WHERE event_kind = 'request';
CREATE INDEX idx_request_records_user_created_at
    ON request_records (user_id, created_at DESC);
CREATE INDEX idx_request_records_user_created_at_record_id
    ON request_records (user_id, created_at DESC, event_id DESC);
CREATE INDEX idx_usage_events_codex_session_candidates
    ON request_records (user_id, path, client_installation_id, created_at DESC);
CREATE INDEX idx_usage_events_conversation_created_at
    ON request_records (conversation_id, created_at DESC);
CREATE INDEX idx_usage_events_request_has_previous_response_id
    ON request_records (request_has_previous_response_id, created_at DESC);

ALTER TABLE request_records
    ADD CONSTRAINT usage_events_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE SET NULL;
ALTER TABLE request_records
    ADD CONSTRAINT usage_events_endpoint_id_fkey
    FOREIGN KEY (endpoint_id) REFERENCES provider_endpoints(endpoint_id) ON DELETE SET NULL;
ALTER TABLE request_records
    ADD CONSTRAINT usage_events_parent_event_id_fkey
    FOREIGN KEY (parent_event_id) REFERENCES request_records(event_id) ON DELETE SET NULL;
ALTER TABLE request_records
    ADD CONSTRAINT usage_events_base_checkpoint_event_id_fkey
    FOREIGN KEY (base_checkpoint_event_id) REFERENCES request_records(event_id) ON DELETE SET NULL;
ALTER TABLE request_records
    ADD CONSTRAINT request_records_client_key_id_fkey
    FOREIGN KEY (client_key_id) REFERENCES client_keys(key_id) ON DELETE SET NULL;
ALTER TABLE request_records
    ADD CONSTRAINT request_records_mcp_server_id_fkey
    FOREIGN KEY (mcp_server_id) REFERENCES mcp_servers(server_id) ON DELETE SET NULL;
ALTER TABLE request_records
    ADD CONSTRAINT request_records_model_route_rule_id_fkey
    FOREIGN KEY (model_route_rule_id) REFERENCES model_endpoint_rules(rule_id) ON DELETE SET NULL;

CREATE TABLE usage_prompt_blocks (
    block_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    content_json JSONB NOT NULL,
    preview_text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
ALTER TABLE usage_prompt_blocks
    ADD CONSTRAINT usage_prompt_blocks_pkey PRIMARY KEY (block_hash);

CREATE TABLE request_record_block_refs (
    event_id BIGINT NOT NULL,
    block_hash TEXT NOT NULL
);
ALTER TABLE request_record_block_refs
    ADD CONSTRAINT request_record_block_refs_pkey PRIMARY KEY (event_id, block_hash);
CREATE INDEX idx_request_record_block_refs_block_hash
    ON request_record_block_refs (block_hash);
ALTER TABLE request_record_block_refs
    ADD CONSTRAINT request_record_block_refs_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES request_records(event_id) ON DELETE CASCADE;
ALTER TABLE request_record_block_refs
    ADD CONSTRAINT request_record_block_refs_block_hash_fkey
    FOREIGN KEY (block_hash) REFERENCES usage_prompt_blocks(block_hash);

CREATE TABLE request_record_assistant_artifacts (
    event_id BIGINT NOT NULL,
    message_json JSONB NOT NULL,
    has_reasoning_content BOOLEAN NOT NULL DEFAULT FALSE,
    has_tool_calls BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
ALTER TABLE request_record_assistant_artifacts
    ADD CONSTRAINT usage_assistant_artifacts_pkey PRIMARY KEY (event_id);
ALTER TABLE request_record_assistant_artifacts
    ADD CONSTRAINT usage_assistant_artifacts_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES request_records(event_id) ON DELETE CASCADE;

CREATE TABLE request_record_tool_calls (
    tool_call_event_id BIGINT NOT NULL
        DEFAULT nextval('request_record_tool_calls_tool_call_event_id_seq'),
    parent_event_id BIGINT NOT NULL,
    conversation_id UUID,
    call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_json JSONB,
    arguments_preview TEXT,
    status TEXT NOT NULL DEFAULT 'emitted',
    sequence_in_turn INTEGER,
    mcp_request_event_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT request_record_tool_calls_pkey PRIMARY KEY (tool_call_event_id);
ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT uq_request_record_tool_calls_parent_call UNIQUE (parent_event_id, call_id);
ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT ck_request_record_tool_calls_status CHECK (
        status IN ('emitted', 'output_received', 'failed', 'skipped')
    );
CREATE INDEX idx_request_record_tool_calls_call_parent
    ON request_record_tool_calls (call_id, parent_event_id);
CREATE INDEX idx_request_record_tool_calls_conversation_status
    ON request_record_tool_calls (conversation_id, status, created_at);
CREATE INDEX idx_request_record_tool_calls_parent
    ON request_record_tool_calls (parent_event_id, sequence_in_turn, tool_call_event_id);
ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT request_record_tool_calls_parent_event_id_fkey
    FOREIGN KEY (parent_event_id) REFERENCES request_records(event_id) ON DELETE CASCADE;
ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT request_record_tool_calls_mcp_request_event_id_fkey
    FOREIGN KEY (mcp_request_event_id) REFERENCES request_records(event_id) ON DELETE SET NULL;

CREATE TABLE request_record_replay_snapshots (
    event_id BIGINT NOT NULL,
    conversation_id UUID NOT NULL,
    conversation_seq INTEGER NOT NULL,
    base_event_id BIGINT NOT NULL,
    prompt_refs_json JSONB NOT NULL,
    ref_count INTEGER NOT NULL,
    byte_size INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
ALTER TABLE request_record_replay_snapshots
    ADD CONSTRAINT request_record_replay_snapshots_pkey PRIMARY KEY (event_id);
CREATE INDEX idx_request_record_replay_snapshots_conversation_seq
    ON request_record_replay_snapshots (conversation_id, conversation_seq DESC, event_id DESC);
ALTER TABLE request_record_replay_snapshots
    ADD CONSTRAINT request_record_replay_snapshots_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES request_records(event_id) ON DELETE CASCADE;
ALTER TABLE request_record_replay_snapshots
    ADD CONSTRAINT request_record_replay_snapshots_base_event_id_fkey
    FOREIGN KEY (base_event_id) REFERENCES request_records(event_id) ON DELETE CASCADE;

-- 4. Restore the loose references that pointed at the family. The request
--    family was dropped without being refilled, so every surviving
--    FK-carrying table is emptied first: with rows still present, adding the
--    constraint back would fail its validation against the now-empty parent
--    and the down path would be unusable. The raw payload parent TRUNCATE
--    cascades to every child partition (the default staging partition and
--    all daily partitions); `_overflow` is an independent staging table and
--    is truncated on its own. These statements run after the parent drop in
--    step 2, so the referencing FKs are already gone and nothing restricts
--    the truncation.
TRUNCATE usage_charges, usage_charge_lines;
TRUNCATE conversation_redaction_sessions;
TRUNCATE request_record_raw_payloads;
TRUNCATE request_record_raw_payloads_overflow;

ALTER TABLE usage_charges
    ADD CONSTRAINT usage_charges_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES request_records(event_id) ON DELETE SET NULL;
ALTER TABLE conversation_redaction_sessions
    ADD CONSTRAINT conversation_redaction_sessions_last_event_id_fkey
    FOREIGN KEY (last_event_id) REFERENCES request_records(event_id) ON DELETE SET NULL;
ALTER TABLE request_record_raw_payloads
    ADD CONSTRAINT request_record_raw_payloads_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES request_records(event_id) ON DELETE CASCADE;
ALTER TABLE request_record_raw_payloads_overflow
    ADD CONSTRAINT request_record_raw_payloads_overflow_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES request_records(event_id) ON DELETE CASCADE;

-- 5. Hand the sequences back to their columns.
ALTER SEQUENCE usage_events_event_id_seq
    OWNED BY request_records.event_id;
ALTER SEQUENCE request_record_tool_calls_tool_call_event_id_seq
    OWNED BY request_record_tool_calls.tool_call_event_id;
