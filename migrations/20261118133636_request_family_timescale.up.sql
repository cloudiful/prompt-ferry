-- Issue #69 P4a: replace the native-partition request family with an empty
-- TimescaleDB eight-hypertable layout, remove the retired billing schema, and
-- drop the retired MCP credential fields.
--
-- This migration is intentionally destructive only for an empty request family
-- and an empty billing chain. A destination that already holds request history
-- refuses before it changes anything: history is not converted, and a fresh
-- destination plus a configuration restore is the only supported cutover.

DO $$
DECLARE
    ext_version TEXT;
    relation TEXT;
    has_rows BOOLEAN;
BEGIN
    SELECT extversion INTO ext_version
    FROM pg_extension
    WHERE extname = 'timescaledb';
    IF ext_version IS NULL THEN
        RAISE EXCEPTION
            'TimescaleDB is required before initializing the request family; install the timescale/timescaledb:2.32.0-pg18 image or CREATE EXTENSION timescaledb once as a superuser';
    END IF;
    IF current_setting('timescaledb.license', true) IS DISTINCT FROM 'timescale' THEN
        RAISE EXCEPTION
            'TimescaleDB license is %, but the columnstore requires the TSL license; use the official non-OSS timescaledb image',
            COALESCE(current_setting('timescaledb.license', true), 'not set');
    END IF;
    IF split_part(ext_version, '.', 1)::INT < 2
        OR (split_part(ext_version, '.', 1)::INT = 2
            AND split_part(ext_version, '.', 2)::INT < 32) THEN
        RAISE EXCEPTION
            'TimescaleDB % is installed, but >= 2.32.0 is required for the native columnstore',
            ext_version;
    END IF;

    FOREACH relation IN ARRAY ARRAY[
        'request_records',
        'request_record_content',
        'request_record_raw_payloads',
        'request_record_block_refs',
        'usage_prompt_blocks',
        'request_record_assistant_artifacts',
        'request_record_tool_calls',
        'request_record_replay_snapshots',
        'request_record_raw_payloads_default',
        'request_record_raw_payloads_overflow'
    ]
    LOOP
        IF to_regclass('public.' || relation) IS NULL THEN
            RAISE EXCEPTION
                'request-family relation public.% is missing; this migration rebuilds the applied native layout',
                relation;
        END IF;
        EXECUTE format('SELECT EXISTS (SELECT 1 FROM public.%I)', relation)
            INTO has_rows;
        IF has_rows THEN
            RAISE EXCEPTION
                'request-family relation public.% is not empty; export the configuration, initialize a fresh database and restore the configuration instead of converting history',
                relation;
        END IF;
    END LOOP;
END
$$;

-- The request-family sequences are independent objects. Detach them before
-- their owners disappear so their heads and defaults survive the rebuild, then
-- reattach them to the replacement columns below.
ALTER SEQUENCE public.usage_events_event_id_seq OWNED BY NONE;
ALTER SEQUENCE public.request_record_tool_calls_tool_call_event_id_seq OWNED BY NONE;

DROP TABLE public.request_record_raw_payloads_overflow;
DROP TABLE public.request_records;
DROP TABLE public.request_record_content;
DROP TABLE public.request_record_raw_payloads;
DROP TABLE public.request_record_block_refs;
DROP TABLE public.usage_prompt_blocks;
DROP TABLE public.request_record_assistant_artifacts;
DROP TABLE public.request_record_tool_calls;
DROP TABLE public.request_record_replay_snapshots;

CREATE TABLE public.request_records (
    event_id BIGINT NOT NULL DEFAULT nextval('public.usage_events_event_id_seq'),
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
    session_parent_id TEXT,
    node_attribution JSONB
);

CREATE TABLE public.request_record_content (
    created_at TIMESTAMPTZ NOT NULL,
    event_id BIGINT NOT NULL,
    request_full_json JSONB,
    request_delta_json JSONB,
    response_prompt TEXT,
    upstream_error_body TEXT
);

CREATE TABLE public.request_record_raw_payloads (
    event_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    request_raw_json JSONB,
    response_raw_body TEXT
);

CREATE TABLE public.request_record_block_refs (
    created_at TIMESTAMPTZ NOT NULL,
    event_id BIGINT NOT NULL,
    block_hash TEXT NOT NULL
);

CREATE TABLE public.usage_prompt_blocks (
    block_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    content_json JSONB NOT NULL,
    preview_text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE public.request_record_assistant_artifacts (
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_id BIGINT NOT NULL,
    message_json JSONB NOT NULL,
    has_reasoning_content BOOLEAN NOT NULL DEFAULT FALSE,
    has_tool_calls BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE public.request_record_tool_calls (
    tool_call_event_id BIGINT NOT NULL
        DEFAULT nextval('public.request_record_tool_calls_tool_call_event_id_seq'),
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
);

CREATE TABLE public.request_record_replay_snapshots (
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    event_id BIGINT NOT NULL,
    conversation_id UUID NOT NULL,
    conversation_seq INTEGER NOT NULL,
    base_event_id BIGINT NOT NULL,
    prompt_refs_json JSONB NOT NULL,
    ref_count INTEGER NOT NULL,
    byte_size INTEGER NOT NULL
);

ALTER TABLE public.request_records
    ADD CONSTRAINT request_records_pkey PRIMARY KEY (event_id, created_at);
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_event_kind CHECK (event_kind = 'request');
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_usage_event_request_storage_mode CHECK (
        request_storage_mode IN ('full', 'append_delta')
    );
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_request_state CHECK (
        request_state IN ('received', 'upstream_processing', 'completed', 'failed', 'aborted')
    );
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_request_category CHECK (request_category IN ('ai', 'mcp'));
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_failure_family CHECK (
        failure_family IS NULL OR failure_family IN (
            'auth', 'rate_limit', 'quota', 'timeout', 'upstream_4xx',
            'upstream_5xx', 'network', 'empty_success', 'policy', 'unknown'
        )
    );
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_route_selection_reason CHECK (
        route_selection_reason IN (
            'default', 'session_affinity', 'session_load_balance', 'conversation_override',
            'quota_failover', 'new_session_preference'
        )
    );
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_mcp_bearer_token_slot CHECK (
        mcp_bearer_token_slot IS NULL OR mcp_bearer_token_slot > 0
    );
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_abort_reason CHECK (
        abort_reason IS NULL OR abort_reason IN (
            'downstream_closed', 'bridge_backpressure_full', 'bridge_backpressure_bytes_limit',
            'worker_lease_expired', 'valkey_lease_missing', 'relay_unknown'
        )
    );
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_abort_from_state CHECK (
        abort_from_state IS NULL OR abort_from_state IN ('received', 'upstream_processing')
    );
ALTER TABLE public.request_records
    ADD CONSTRAINT ck_request_records_applied_thinking_effort CHECK (
        applied_thinking_effort_override IS NULL OR applied_thinking_effort_override IN (
            'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'
        )
    );

CREATE UNIQUE INDEX uq_request_records_request_id_request
    ON public.request_records (request_id, created_at)
    WHERE event_kind = 'request';
CREATE INDEX idx_request_records_created_at
    ON public.request_records (created_at DESC);
CREATE INDEX idx_request_records_user_created_at
    ON public.request_records (user_id, created_at DESC);
CREATE INDEX idx_request_records_client_key_created_at
    ON public.request_records (client_key_id, created_at DESC);
CREATE INDEX idx_request_records_conversation_seq
    ON public.request_records (conversation_id, conversation_seq DESC);
CREATE INDEX idx_request_records_provider_response_id
    ON public.request_records (provider_response_id);
CREATE INDEX idx_request_records_provider_conversation_key
    ON public.request_records (provider_conversation_key, user_id, event_id DESC);
CREATE INDEX idx_request_records_usage_covering
    ON public.request_records (request_category, created_at DESC)
    INCLUDE (
        user_id, ok, request_state, duration_ms, ttft_ms, input_tokens, output_tokens,
        total_tokens, cached_tokens, cache_read_tokens, cache_write_tokens, mcp_protocol_method,
        model, endpoint_id, failure_family, mcp_server_id, mcp_server_name, client_key_id,
        client_key_label
    )
    WHERE event_kind = 'request';

ALTER TABLE public.request_record_content
    ADD CONSTRAINT request_record_content_pkey PRIMARY KEY (event_id, created_at);

ALTER TABLE public.request_record_raw_payloads
    ADD CONSTRAINT request_record_raw_payloads_pkey PRIMARY KEY (created_at, event_id);
ALTER TABLE public.request_record_raw_payloads
    ADD CONSTRAINT ck_request_record_raw_payloads_has_payload CHECK (
        request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL
    );
CREATE INDEX idx_request_record_raw_payloads_created_at_event_id
    ON public.request_record_raw_payloads (created_at, event_id);
CREATE INDEX idx_request_record_raw_payloads_event_id
    ON public.request_record_raw_payloads (event_id);

ALTER TABLE public.request_record_block_refs
    ADD CONSTRAINT request_record_block_refs_pkey PRIMARY KEY (event_id, block_hash, created_at);
CREATE INDEX idx_request_record_block_refs_block_hash
    ON public.request_record_block_refs (block_hash);

ALTER TABLE public.usage_prompt_blocks
    ADD CONSTRAINT usage_prompt_blocks_pkey PRIMARY KEY (block_hash, created_at);

ALTER TABLE public.request_record_assistant_artifacts
    ADD CONSTRAINT request_record_assistant_artifacts_pkey PRIMARY KEY (event_id, created_at);

ALTER TABLE public.request_record_tool_calls
    ADD CONSTRAINT request_record_tool_calls_pkey PRIMARY KEY (tool_call_event_id, created_at);
ALTER TABLE public.request_record_tool_calls
    ADD CONSTRAINT uq_request_record_tool_calls_parent_call UNIQUE (parent_event_id, call_id, created_at);
ALTER TABLE public.request_record_tool_calls
    ADD CONSTRAINT ck_request_record_tool_calls_status CHECK (
        status IN ('emitted', 'output_received', 'failed', 'skipped')
    );
CREATE INDEX idx_request_record_tool_calls_parent
    ON public.request_record_tool_calls (parent_event_id, sequence_in_turn, tool_call_event_id);
CREATE INDEX idx_request_record_tool_calls_call_parent
    ON public.request_record_tool_calls (call_id, parent_event_id);

ALTER TABLE public.request_record_replay_snapshots
    ADD CONSTRAINT request_record_replay_snapshots_pkey PRIMARY KEY (event_id, created_at);
CREATE INDEX idx_request_record_replay_snapshots_conversation_seq
    ON public.request_record_replay_snapshots (conversation_id, conversation_seq DESC, event_id DESC);

ALTER SEQUENCE public.usage_events_event_id_seq
    OWNED BY public.request_records.event_id;
ALTER SEQUENCE public.request_record_tool_calls_tool_call_event_id_seq
    OWNED BY public.request_record_tool_calls.tool_call_event_id;

SELECT create_hypertable(
    'public.request_records',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);
SELECT create_hypertable(
    'public.request_record_content',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);
SELECT create_hypertable(
    'public.request_record_raw_payloads',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);
SELECT create_hypertable(
    'public.request_record_block_refs',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);
SELECT create_hypertable(
    'public.usage_prompt_blocks',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);
SELECT create_hypertable(
    'public.request_record_assistant_artifacts',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);
SELECT create_hypertable(
    'public.request_record_tool_calls',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);
SELECT create_hypertable(
    'public.request_record_replay_snapshots',
    'created_at',
    chunk_time_interval => INTERVAL '1 day',
    create_default_indexes => FALSE,
    migrate_data => FALSE
);

-- Columnstore for every hypertable. `created_at DESC` is the only ordering
-- column: no segment-by on the unique event identifiers, and no content or raw
-- payload segment columns. `request_category` is not used as a segment column
-- because no measured workload justifies the extra chunk granularity yet.
ALTER TABLE public.request_records
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');
ALTER TABLE public.request_record_content
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');
ALTER TABLE public.request_record_raw_payloads
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');
ALTER TABLE public.request_record_block_refs
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');
ALTER TABLE public.usage_prompt_blocks
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');
ALTER TABLE public.request_record_assistant_artifacts
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');
ALTER TABLE public.request_record_tool_calls
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');
ALTER TABLE public.request_record_replay_snapshots
    SET (timescaledb.enable_columnstore = TRUE, timescaledb.orderby = 'created_at DESC');

-- A one-day chunk is only closed after the following boundary, so a chunk is
-- compressed roughly 24 to 48 hours after its first row, never at an exact age.
CALL add_columnstore_policy('public.request_records', INTERVAL '1 day');
CALL add_columnstore_policy('public.request_record_content', INTERVAL '1 day');
CALL add_columnstore_policy('public.request_record_raw_payloads', INTERVAL '1 day');
CALL add_columnstore_policy('public.request_record_block_refs', INTERVAL '1 day');
CALL add_columnstore_policy('public.usage_prompt_blocks', INTERVAL '1 day');
CALL add_columnstore_policy('public.request_record_assistant_artifacts', INTERVAL '1 day');
CALL add_columnstore_policy('public.request_record_tool_calls', INTERVAL '1 day');
CALL add_columnstore_policy('public.request_record_replay_snapshots', INTERVAL '1 day');

-- The billing feature is retired in full. Child tables go before their parents
-- so no unrelated object is cascaded.
DROP TABLE IF EXISTS public.usage_charge_adjustments;
DROP TABLE public.usage_charge_lines;
DROP TABLE public.usage_charges;
DROP TABLE public.billing_price_rules;

-- Retired MCP credential fields: remove the two checks, replace the update
-- trigger with the six remaining configuration columns, then drop the columns.
ALTER TABLE public.mcp_credentials
    DROP CONSTRAINT IF EXISTS ck_mcp_credentials_default_cost;
ALTER TABLE public.mcp_credentials
    DROP CONSTRAINT IF EXISTS ck_mcp_credentials_period;
DROP TRIGGER IF EXISTS trg_cfg_rev_mcp_credentials_update ON public.mcp_credentials;
CREATE TRIGGER trg_cfg_rev_mcp_credentials_update
AFTER UPDATE OF server_id, credential_label, secret, position, enabled, provider_kind ON public.mcp_credentials
FOR EACH ROW
WHEN (
    old.server_id IS DISTINCT FROM new.server_id
    OR old.credential_label IS DISTINCT FROM new.credential_label
    OR old.secret IS DISTINCT FROM new.secret
    OR old.position IS DISTINCT FROM new.position
    OR old.enabled IS DISTINCT FROM new.enabled
    OR old.provider_kind IS DISTINCT FROM new.provider_kind
)
EXECUTE FUNCTION public.prompt_ferry_bump_config_revision();

ALTER TABLE public.mcp_credentials
    DROP COLUMN default_cost,
    DROP COLUMN strict_mode,
    DROP COLUMN billing_period_start,
    DROP COLUMN billing_period_end;
