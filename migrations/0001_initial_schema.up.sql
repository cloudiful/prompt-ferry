CREATE TABLE IF NOT EXISTS users (
    user_id BIGSERIAL PRIMARY KEY,
    login_name TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    is_admin BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS client_keys (
    key_id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_client_keys_user ON client_keys(user_id, enabled, created_at DESC);

CREATE TABLE IF NOT EXISTS provider_endpoints (
    endpoint_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
    owner_user_id BIGINT REFERENCES users(user_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    native_api TEXT NOT NULL DEFAULT 'chat' CHECK (native_api IN ('responses', 'chat')),
    native_api_source TEXT NOT NULL DEFAULT 'manual' CHECK (native_api_source IN ('detected', 'manual')),
    api_key TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_provider_endpoint_owner CHECK (
        (scope = 'admin' AND owner_user_id IS NULL)
        OR (scope = 'user' AND owner_user_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_provider_endpoints_scope_user ON provider_endpoints(scope, owner_user_id, enabled);

ALTER TABLE provider_endpoints ADD COLUMN IF NOT EXISTS native_api TEXT NOT NULL DEFAULT 'chat';
ALTER TABLE provider_endpoints ADD COLUMN IF NOT EXISTS native_api_source TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE provider_endpoints DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api;
ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api CHECK (native_api IN ('responses', 'chat'));
ALTER TABLE provider_endpoints DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api_source;
ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api_source CHECK (native_api_source IN ('detected', 'manual'));

CREATE TABLE IF NOT EXISTS user_endpoint_settings (
    user_id BIGINT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    endpoint_id UUID REFERENCES provider_endpoints(endpoint_id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS worker_settings (
    setting_key TEXT PRIMARY KEY,
    setting_value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS approval_requests (
    approval_id UUID PRIMARY KEY,
    request_id UUID NOT NULL UNIQUE,
    user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    client_key_label TEXT,
    path TEXT NOT NULL,
    model TEXT,
    review_decision TEXT NOT NULL,
    approval_status TEXT NOT NULL,
    review_reason TEXT NOT NULL DEFAULT '',
    review_categories TEXT[] NOT NULL DEFAULT '{}'::TEXT[],
    request_preview TEXT NOT NULL,
    request_payload_json JSONB,
    request_deadline_unix_ms BIGINT NOT NULL DEFAULT 0,
    wait_deadline_unix_ms BIGINT NOT NULL DEFAULT 0,
    decided_by_user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    decided_at TIMESTAMPTZ,
    webhook_last_error TEXT,
    webhook_last_attempted_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE approval_requests ADD COLUMN IF NOT EXISTS request_deadline_unix_ms BIGINT NOT NULL DEFAULT 0;
ALTER TABLE approval_requests ADD COLUMN IF NOT EXISTS wait_deadline_unix_ms BIGINT NOT NULL DEFAULT 0;
ALTER TABLE approval_requests ADD COLUMN IF NOT EXISTS webhook_last_error TEXT;
ALTER TABLE approval_requests ADD COLUMN IF NOT EXISTS webhook_last_attempted_at TIMESTAMPTZ;
ALTER TABLE approval_requests DROP CONSTRAINT IF EXISTS ck_approval_requests_review_decision;
ALTER TABLE approval_requests
ADD CONSTRAINT ck_approval_requests_review_decision
CHECK (review_decision IN ('allow', 'flag', 'error', 'timeout'));
ALTER TABLE approval_requests DROP CONSTRAINT IF EXISTS ck_approval_requests_approval_status;
ALTER TABLE approval_requests
ADD CONSTRAINT ck_approval_requests_approval_status
CHECK (approval_status IN ('pending', 'approved', 'rejected', 'expired', 'aborted'));

CREATE INDEX IF NOT EXISTS idx_approval_requests_status_created_at
ON approval_requests(approval_status, created_at DESC);

CREATE TABLE IF NOT EXISTS usage_prompt_blocks (
    block_hash TEXT PRIMARY KEY,
    role TEXT NOT NULL,
    content_json JSONB NOT NULL,
    preview_text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS usage_events (
    event_id BIGSERIAL PRIMARY KEY,
    request_id UUID NOT NULL,
    user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    client_key_label TEXT,
    endpoint_id UUID REFERENCES provider_endpoints(endpoint_id) ON DELETE SET NULL,
    path TEXT NOT NULL,
    model TEXT,
    status INTEGER,
    ok BOOLEAN NOT NULL,
    duration_ms BIGINT NOT NULL,
    first_chunk_ms BIGINT,
    input_tokens BIGINT,
    output_tokens BIGINT,
    total_tokens BIGINT,
    cached_tokens BIGINT,
    cache_read_tokens BIGINT,
    cache_write_tokens BIGINT,
    conversation_id UUID,
    parent_event_id BIGINT REFERENCES usage_events(event_id) ON DELETE SET NULL,
    conversation_seq INTEGER,
    request_storage_mode TEXT NOT NULL DEFAULT 'full',
    request_full_json JSONB,
    request_delta_json JSONB,
    request_full_text TEXT,
    request_delta_text TEXT,
    provider_response_id TEXT,
    base_checkpoint_event_id BIGINT REFERENCES usage_events(event_id) ON DELETE SET NULL,
    request_prompt TEXT,
    response_prompt TEXT,
    upstream_error_body TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS usage_assistant_artifacts (
    event_id BIGINT PRIMARY KEY REFERENCES usage_events(event_id) ON DELETE CASCADE,
    message_json JSONB NOT NULL,
    has_reasoning_content BOOLEAN NOT NULL DEFAULT FALSE,
    has_tool_calls BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS first_chunk_ms BIGINT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS cached_tokens BIGINT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS cache_read_tokens BIGINT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS cache_write_tokens BIGINT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS request_prompt TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS client_key_label TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS response_prompt TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS upstream_error_body TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS conversation_id UUID;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS parent_event_id BIGINT REFERENCES usage_events(event_id) ON DELETE SET NULL;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS conversation_seq INTEGER;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS request_storage_mode TEXT NOT NULL DEFAULT 'full';
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS request_full_json JSONB;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS request_delta_json JSONB;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS request_full_text TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS request_delta_text TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS provider_response_id TEXT;
ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS base_checkpoint_event_id BIGINT REFERENCES usage_events(event_id) ON DELETE SET NULL;
ALTER TABLE usage_events DROP CONSTRAINT IF EXISTS ck_usage_event_request_storage_mode;
ALTER TABLE usage_events
ADD CONSTRAINT ck_usage_event_request_storage_mode CHECK (request_storage_mode IN ('full', 'append_delta'));

CREATE INDEX IF NOT EXISTS idx_usage_events_created_at ON usage_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_created_at_event_id
ON usage_events(created_at DESC, event_id DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_user_created_at ON usage_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_user_created_at_event_id
ON usage_events(user_id, created_at DESC, event_id DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_endpoint_created_at ON usage_events(endpoint_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_request_id ON usage_events(request_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_conversation_seq ON usage_events(conversation_id, conversation_seq DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_provider_response_id ON usage_events(provider_response_id);

CREATE TABLE IF NOT EXISTS model_endpoint_rules (
    rule_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
    owner_user_id BIGINT REFERENCES users(user_id) ON DELETE CASCADE,
    model_pattern TEXT NOT NULL,
    endpoint_id UUID NOT NULL REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    priority INTEGER NOT NULL DEFAULT 100,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_model_route_owner CHECK (
        (scope = 'admin' AND owner_user_id IS NULL)
        OR (scope = 'user' AND owner_user_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_model_endpoint_rules_scope_user
ON model_endpoint_rules(scope, owner_user_id, enabled, priority);

CREATE TABLE IF NOT EXISTS model_route_targets (
    target_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    rule_id UUID NOT NULL REFERENCES model_endpoint_rules(rule_id) ON DELETE CASCADE,
    endpoint_id UUID NOT NULL REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    responses_passthrough BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_route_targets_rule_position ON model_route_targets(rule_id, position);
CREATE INDEX IF NOT EXISTS idx_model_route_targets_endpoint_enabled ON model_route_targets(endpoint_id, enabled);

INSERT INTO model_route_targets(rule_id, endpoint_id, position, enabled)
SELECT r.rule_id, r.endpoint_id, 0, TRUE
FROM model_endpoint_rules r
WHERE NOT EXISTS (
    SELECT 1
    FROM model_route_targets t
    WHERE t.rule_id = r.rule_id
);

ALTER TABLE model_route_targets ADD COLUMN IF NOT EXISTS responses_passthrough BOOLEAN;
UPDATE model_route_targets SET responses_passthrough = FALSE WHERE responses_passthrough IS NULL;
ALTER TABLE model_route_targets ALTER COLUMN responses_passthrough SET DEFAULT FALSE;
ALTER TABLE model_route_targets ALTER COLUMN responses_passthrough SET NOT NULL;

CREATE TABLE IF NOT EXISTS mcp_servers (
    server_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    scope TEXT NOT NULL DEFAULT 'admin' CHECK (scope IN ('admin', 'user')),
    owner_user_id BIGINT REFERENCES users(user_id) ON DELETE CASCADE,
    name TEXT NOT NULL UNIQUE,
    transport TEXT NOT NULL CHECK (transport IN ('http', 'stdio')),
    url TEXT,
    command TEXT,
    args JSONB NOT NULL DEFAULT '[]'::JSONB,
    env_json JSONB NOT NULL DEFAULT '{}'::JSONB,
    disabled_tools JSONB NOT NULL DEFAULT '[]'::JSONB,
    disabled_resources JSONB NOT NULL DEFAULT '[]'::JSONB,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    timeout_ms INTEGER NOT NULL DEFAULT 30000,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_mcp_server_owner CHECK (
        (scope = 'admin' AND owner_user_id IS NULL)
        OR (scope = 'user' AND owner_user_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled, name);
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS scope TEXT;
UPDATE mcp_servers SET scope = 'admin' WHERE scope IS NULL;
ALTER TABLE mcp_servers ALTER COLUMN scope SET DEFAULT 'admin';
ALTER TABLE mcp_servers ALTER COLUMN scope SET NOT NULL;
ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_scope;
ALTER TABLE mcp_servers ADD CONSTRAINT ck_mcp_server_scope CHECK (scope IN ('admin', 'user'));
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS owner_user_id BIGINT REFERENCES users(user_id) ON DELETE CASCADE;
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS disabled_tools JSONB NOT NULL DEFAULT '[]'::JSONB;
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS disabled_resources JSONB NOT NULL DEFAULT '[]'::JSONB;
ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_owner;
ALTER TABLE mcp_servers ADD CONSTRAINT ck_mcp_server_owner CHECK (
    (scope = 'admin' AND owner_user_id IS NULL)
    OR (scope = 'user' AND owner_user_id IS NOT NULL)
);
CREATE INDEX IF NOT EXISTS idx_mcp_servers_scope_user_enabled ON mcp_servers(scope, owner_user_id, enabled, name);
