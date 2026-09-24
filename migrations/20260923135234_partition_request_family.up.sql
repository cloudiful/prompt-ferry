-- Issue #277 Phase P7 — request family rebuilt as daily RANGE(created_at)
-- partitions with a metadata/content split.
--
-- Destructive by design (operator decision 2026-09-23): rows outside the
-- retention window are dropped instead of copied, both directions accept data
-- loss, and every cross-family foreign key is removed so partition drops stay
-- metadata-only. Application-level integrity replaces the FKs: family rows
-- written for the same event share the same `created_at` day.
--
-- Metadata retention is 90 days, content-family retention is 3 days. The
-- partition manager that maintains the horizon is Phase P8; this migration
-- pre-creates the window it hands over.

-- 1. Owned sequences must survive the table swap below.
ALTER SEQUENCE usage_events_event_id_seq OWNED BY NONE;
ALTER SEQUENCE request_record_tool_calls_tool_call_event_id_seq OWNED BY NONE;

-- 2. Park the legacy tables so the new parents can take the real names.
ALTER TABLE request_records RENAME TO request_records_legacy;
ALTER TABLE request_record_block_refs RENAME TO request_record_block_refs_legacy;
ALTER TABLE usage_prompt_blocks RENAME TO usage_prompt_blocks_legacy;
ALTER TABLE request_record_assistant_artifacts
    RENAME TO request_record_assistant_artifacts_legacy;
ALTER TABLE request_record_tool_calls RENAME TO request_record_tool_calls_legacy;
ALTER TABLE request_record_replay_snapshots
    RENAME TO request_record_replay_snapshots_legacy;

-- 3. New parents. Constraints and indexes are added after the legacy tables
--    are gone (index names are schema-wide).
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
    provider_response_id TEXT,
    base_checkpoint_event_id BIGINT,
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
    abort_reason TEXT,
    abort_from_state TEXT,
    abort_response_started BOOLEAN,
    applied_thinking_effort_override TEXT,
    session_header_id TEXT,
    session_parent_id TEXT
) PARTITION BY RANGE (created_at);

-- Content family: same `event_id`, daily partitions, 3-day lifecycle.
CREATE TABLE request_record_content (
    created_at TIMESTAMPTZ NOT NULL,
    event_id BIGINT NOT NULL,
    request_full_json JSONB,
    request_delta_json JSONB,
    response_prompt TEXT,
    upstream_error_body TEXT
) PARTITION BY RANGE (created_at);

CREATE TABLE request_record_block_refs (
    created_at TIMESTAMPTZ NOT NULL,
    event_id BIGINT NOT NULL,
    block_hash TEXT NOT NULL
) PARTITION BY RANGE (created_at);

CREATE TABLE usage_prompt_blocks (
    block_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    content_json JSONB NOT NULL,
    preview_text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
) PARTITION BY RANGE (created_at);

CREATE TABLE request_record_assistant_artifacts (
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_id BIGINT NOT NULL,
    message_json JSONB NOT NULL,
    has_reasoning_content BOOLEAN NOT NULL DEFAULT FALSE,
    has_tool_calls BOOLEAN NOT NULL DEFAULT FALSE
) PARTITION BY RANGE (created_at);

CREATE TABLE request_record_tool_calls (
    tool_call_event_id BIGINT NOT NULL
        DEFAULT nextval('request_record_tool_calls_tool_call_event_id_seq'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    parent_event_id BIGINT NOT NULL,
    conversation_id UUID,
    call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_json JSONB,
    arguments_preview TEXT,
    status TEXT NOT NULL DEFAULT 'emitted',
    sequence_in_turn INTEGER,
    mcp_request_event_id BIGINT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
) PARTITION BY RANGE (created_at);

CREATE TABLE request_record_replay_snapshots (
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_id BIGINT NOT NULL,
    conversation_id UUID NOT NULL,
    conversation_seq INTEGER NOT NULL,
    base_event_id BIGINT NOT NULL,
    prompt_refs_json JSONB NOT NULL,
    ref_count INTEGER NOT NULL,
    byte_size INTEGER NOT NULL
) PARTITION BY RANGE (created_at);

-- 4. Daily partitions: every day that actually carries rows plus the handover
--    window (metadata 90-day retention with slack, content family 3 days).
CREATE TEMP TABLE p7_partition_days (day DATE PRIMARY KEY) ON COMMIT DROP;

INSERT INTO p7_partition_days (day)
SELECT DISTINCT (created_at AT TIME ZONE 'UTC')::DATE
FROM request_records_legacy
UNION
SELECT ((NOW() AT TIME ZONE 'UTC')::DATE + offset_days)
FROM generate_series(-95, 8) AS offset_days;

INSERT INTO p7_partition_days (day)
SELECT day
FROM (
    SELECT DISTINCT (records.created_at AT TIME ZONE 'UTC')::DATE AS day
    FROM request_record_block_refs_legacy refs
    JOIN request_records_legacy records ON records.event_id = refs.event_id
    UNION
    SELECT DISTINCT (created_at AT TIME ZONE 'UTC')::DATE
    FROM usage_prompt_blocks_legacy
    UNION
    SELECT DISTINCT (created_at AT TIME ZONE 'UTC')::DATE
    FROM request_record_assistant_artifacts_legacy
    UNION
    SELECT DISTINCT (created_at AT TIME ZONE 'UTC')::DATE
    FROM request_record_tool_calls_legacy
    UNION
    SELECT DISTINCT (created_at AT TIME ZONE 'UTC')::DATE
    FROM request_record_replay_snapshots_legacy
) AS content_days
ON CONFLICT (day) DO NOTHING;

DO $$
DECLARE
    entry RECORD;
    partition_day DATE;
    start_at TEXT;
    end_at TEXT;
BEGIN
    FOR entry IN
        SELECT *
        FROM (
            VALUES
                ('request_records', 95),
                ('request_record_content', 8),
                ('request_record_block_refs', 8),
                ('usage_prompt_blocks', 8),
                ('request_record_assistant_artifacts', 8),
                ('request_record_tool_calls', 8),
                ('request_record_replay_snapshots', 8)
        ) AS tables(target, lookback_days)
    LOOP
        FOR partition_day IN
            SELECT days.day
            FROM p7_partition_days days
            WHERE days.day >= (NOW() AT TIME ZONE 'UTC')::DATE - entry.lookback_days
        LOOP
            start_at := to_char(partition_day, 'YYYY-MM-DD') || ' 00:00:00+00';
            end_at := to_char(partition_day + 1, 'YYYY-MM-DD') || ' 00:00:00+00';
            EXECUTE format(
                'CREATE TABLE IF NOT EXISTS %I PARTITION OF %I FOR VALUES FROM (%L) TO (%L)',
                entry.target || '_' || to_char(partition_day, 'YYYYMMDD'),
                entry.target,
                start_at,
                end_at
            );
        END LOOP;
    END LOOP;
END $$;

-- 5. Carry over the in-window rows only.
INSERT INTO request_records (
    event_id,
    request_id,
    user_id,
    client_key_label,
    endpoint_id,
    path,
    model,
    status,
    ok,
    duration_ms,
    ttft_ms,
    input_tokens,
    output_tokens,
    total_tokens,
    cached_tokens,
    cache_read_tokens,
    cache_write_tokens,
    conversation_id,
    parent_event_id,
    conversation_seq,
    request_storage_mode,
    provider_response_id,
    base_checkpoint_event_id,
    error_code,
    error_message,
    created_at,
    request_user_agent,
    request_has_previous_response_id,
    request_previous_response_id,
    request_previous_response_parent_found,
    client_installation_id,
    normalized_item_count,
    normalized_chain_hash,
    normalized_first_ref_hash,
    normalized_last_ref_hash,
    conversation_source,
    event_kind,
    request_state,
    updated_at,
    storage_sanitized,
    storage_sanitized_nul_count,
    request_category,
    model_route_rule_id,
    mcp_server_id,
    mcp_server_name,
    mcp_protocol_method,
    mcp_operation_name,
    failure_family,
    mcp_bearer_token_slot,
    route_selection_reason,
    owner_worker_id,
    lease_expires_at,
    last_heartbeat_at,
    http_request_content_encoding,
    http_request_compressed,
    http_request_compressed_bytes,
    http_request_decompressed_bytes,
    http_request_compression_ratio,
    provider_conversation_key,
    request_conversation_parent_found,
    request_conversation_key,
    redaction_applied,
    redaction_findings_count,
    redaction_replacements_count,
    redaction_types_json,
    redaction_fields_json,
    upstream_redaction_enabled,
    endpoint_key_id,
    endpoint_key_label,
    response_capture_truncated,
    client_key_id,
    requested_model,
    upstream_model,
    abort_reason,
    abort_from_state,
    abort_response_started,
    applied_thinking_effort_override,
    session_header_id,
    session_parent_id
)
SELECT
    event_id,
    request_id,
    user_id,
    client_key_label,
    endpoint_id,
    path,
    model,
    status,
    ok,
    duration_ms,
    ttft_ms,
    input_tokens,
    output_tokens,
    total_tokens,
    cached_tokens,
    cache_read_tokens,
    cache_write_tokens,
    conversation_id,
    parent_event_id,
    conversation_seq,
    request_storage_mode,
    provider_response_id,
    base_checkpoint_event_id,
    error_code,
    error_message,
    created_at,
    request_user_agent,
    request_has_previous_response_id,
    request_previous_response_id,
    request_previous_response_parent_found,
    client_installation_id,
    normalized_item_count,
    normalized_chain_hash,
    normalized_first_ref_hash,
    normalized_last_ref_hash,
    conversation_source,
    event_kind,
    request_state,
    updated_at,
    storage_sanitized,
    storage_sanitized_nul_count,
    request_category,
    model_route_rule_id,
    mcp_server_id,
    mcp_server_name,
    mcp_protocol_method,
    mcp_operation_name,
    failure_family,
    mcp_bearer_token_slot,
    route_selection_reason,
    owner_worker_id,
    lease_expires_at,
    last_heartbeat_at,
    http_request_content_encoding,
    http_request_compressed,
    http_request_compressed_bytes,
    http_request_decompressed_bytes,
    http_request_compression_ratio,
    provider_conversation_key,
    request_conversation_parent_found,
    request_conversation_key,
    redaction_applied,
    redaction_findings_count,
    redaction_replacements_count,
    redaction_types_json,
    redaction_fields_json,
    upstream_redaction_enabled,
    endpoint_key_id,
    endpoint_key_label,
    response_capture_truncated,
    client_key_id,
    requested_model,
    upstream_model,
    abort_reason,
    abort_from_state,
    abort_response_started,
    applied_thinking_effort_override,
    session_header_id,
    session_parent_id
FROM request_records_legacy
WHERE created_at >= NOW() - INTERVAL '90 days';

-- Content rows exist even when every content column is NULL: a missing row is
-- the new "expired" signal, so a request without payloads still owns a row.
INSERT INTO request_record_content (
    created_at,
    event_id,
    request_full_json,
    request_delta_json,
    response_prompt,
    upstream_error_body
)
SELECT
    created_at,
    event_id,
    request_full_json,
    request_delta_json,
    response_prompt,
    upstream_error_body
FROM request_records_legacy
WHERE created_at >= NOW() - INTERVAL '3 days';

INSERT INTO request_record_block_refs (created_at, event_id, block_hash)
SELECT records.created_at, refs.event_id, refs.block_hash
FROM request_record_block_refs_legacy refs
JOIN request_records_legacy records ON records.event_id = refs.event_id
WHERE records.created_at >= NOW() - INTERVAL '3 days';

-- Prompt blocks are content-addressed per day; rows older than the content
-- window are dropped and re-created on the next write that references them.
INSERT INTO usage_prompt_blocks (block_hash, role, content_json, preview_text, created_at)
SELECT block_hash, role, content_json, preview_text, created_at
FROM usage_prompt_blocks_legacy
WHERE created_at >= NOW() - INTERVAL '3 days';

INSERT INTO request_record_assistant_artifacts (
    created_at,
    event_id,
    message_json,
    has_reasoning_content,
    has_tool_calls
)
SELECT
    created_at,
    event_id,
    message_json,
    has_reasoning_content,
    has_tool_calls
FROM request_record_assistant_artifacts_legacy
WHERE created_at >= NOW() - INTERVAL '3 days';

INSERT INTO request_record_tool_calls (
    tool_call_event_id,
    created_at,
    parent_event_id,
    conversation_id,
    call_id,
    tool_name,
    arguments_json,
    arguments_preview,
    status,
    sequence_in_turn,
    mcp_request_event_id,
    updated_at
)
SELECT
    tool_call_event_id,
    created_at,
    parent_event_id,
    conversation_id,
    call_id,
    tool_name,
    arguments_json,
    arguments_preview,
    status,
    sequence_in_turn,
    mcp_request_event_id,
    updated_at
FROM request_record_tool_calls_legacy
WHERE created_at >= NOW() - INTERVAL '3 days';

INSERT INTO request_record_replay_snapshots (
    created_at,
    event_id,
    conversation_id,
    conversation_seq,
    base_event_id,
    prompt_refs_json,
    ref_count,
    byte_size
)
SELECT
    created_at,
    event_id,
    conversation_id,
    conversation_seq,
    base_event_id,
    prompt_refs_json,
    ref_count,
    byte_size
FROM request_record_replay_snapshots_legacy
WHERE created_at >= NOW() - INTERVAL '3 days';

-- 6. Legacy tables and every foreign key that referenced them disappear.
DROP TABLE request_records_legacy CASCADE;
DROP TABLE request_record_block_refs_legacy CASCADE;
DROP TABLE usage_prompt_blocks_legacy CASCADE;
DROP TABLE request_record_assistant_artifacts_legacy CASCADE;
DROP TABLE request_record_tool_calls_legacy CASCADE;
DROP TABLE request_record_replay_snapshots_legacy CASCADE;

-- Kept tables stay; their references become loose.
ALTER TABLE usage_charges
    DROP CONSTRAINT IF EXISTS usage_charges_event_id_fkey;
ALTER TABLE conversation_redaction_sessions
    DROP CONSTRAINT IF EXISTS conversation_redaction_sessions_last_event_id_fkey;

-- 7. Keys and indexes on the new parents. `request_records` keeps eight
--    secondary indexes: the facets/list covering index, the three list
--    filters that lead with a window, the two replay locators, the request
--    id arbiter, and the created_at maintenance range scan.
ALTER TABLE request_records
    ADD CONSTRAINT request_records_pkey PRIMARY KEY (event_id, created_at);
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
    ON request_records (request_id, created_at)
    WHERE event_kind = 'request';
CREATE INDEX idx_request_records_created_at
    ON request_records (created_at DESC);
CREATE INDEX idx_request_records_user_created_at
    ON request_records (user_id, created_at DESC);
CREATE INDEX idx_request_records_client_key_created_at
    ON request_records (client_key_id, created_at DESC);
CREATE INDEX idx_request_records_conversation_seq
    ON request_records (conversation_id, conversation_seq DESC);
CREATE INDEX idx_request_records_provider_response_id
    ON request_records (provider_response_id);
CREATE INDEX idx_request_records_provider_conversation_key
    ON request_records (provider_conversation_key, user_id, event_id DESC);
CREATE INDEX idx_request_records_usage_covering
    ON request_records (request_category, created_at DESC)
    INCLUDE (
        user_id, ok, request_state, duration_ms, ttft_ms, input_tokens, output_tokens,
        total_tokens, cached_tokens, cache_read_tokens, cache_write_tokens, mcp_protocol_method,
        model, endpoint_id, failure_family, mcp_server_id, mcp_server_name, client_key_id,
        client_key_label
    )
    WHERE event_kind = 'request';

ALTER TABLE request_record_content
    ADD CONSTRAINT request_record_content_pkey PRIMARY KEY (event_id, created_at);

ALTER TABLE request_record_block_refs
    ADD CONSTRAINT request_record_block_refs_pkey PRIMARY KEY (event_id, block_hash, created_at);
CREATE INDEX idx_request_record_block_refs_block_hash
    ON request_record_block_refs (block_hash);

ALTER TABLE usage_prompt_blocks
    ADD CONSTRAINT usage_prompt_blocks_pkey PRIMARY KEY (block_hash, created_at);

ALTER TABLE request_record_assistant_artifacts
    ADD CONSTRAINT request_record_assistant_artifacts_pkey PRIMARY KEY (event_id, created_at);

ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT request_record_tool_calls_pkey PRIMARY KEY (tool_call_event_id, created_at);
ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT uq_request_record_tool_calls_parent_call UNIQUE (parent_event_id, call_id, created_at);
ALTER TABLE request_record_tool_calls
    ADD CONSTRAINT ck_request_record_tool_calls_status CHECK (
        status IN ('emitted', 'output_received', 'failed', 'skipped')
    );
CREATE INDEX idx_request_record_tool_calls_parent
    ON request_record_tool_calls (parent_event_id, sequence_in_turn, tool_call_event_id);
CREATE INDEX idx_request_record_tool_calls_call_parent
    ON request_record_tool_calls (call_id, parent_event_id);

ALTER TABLE request_record_replay_snapshots
    ADD CONSTRAINT request_record_replay_snapshots_pkey PRIMARY KEY (event_id, created_at);
CREATE INDEX idx_request_record_replay_snapshots_conversation_seq
    ON request_record_replay_snapshots (conversation_id, conversation_seq DESC, event_id DESC);

-- 8. Hand the sequences back to their columns.
ALTER SEQUENCE usage_events_event_id_seq
    OWNED BY request_records.event_id;
ALTER SEQUENCE request_record_tool_calls_tool_call_event_id_seq
    OWNED BY request_record_tool_calls.tool_call_event_id;

-- 9. Fresh statistics for the swapped tables.
ANALYZE request_records;
ANALYZE request_record_content;
ANALYZE request_record_block_refs;
ANALYZE usage_prompt_blocks;
ANALYZE request_record_assistant_artifacts;
ANALYZE request_record_tool_calls;
ANALYZE request_record_replay_snapshots;
