CREATE TABLE IF NOT EXISTS standalone_schema_meta (
    schema_key TEXT PRIMARY KEY CHECK (schema_key = 'standalone'),
    schema_version INTEGER NOT NULL
);

INSERT INTO standalone_schema_meta(schema_key, schema_version)
VALUES ('standalone', 1)
ON CONFLICT(schema_key) DO NOTHING;

CREATE TABLE IF NOT EXISTS standalone_relays (
    relay_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    relay_url TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    tls_mode TEXT NOT NULL CHECK (tls_mode IN ('off', 'server', 'mtls')),
    bridge_encryption_mode TEXT NOT NULL CHECK (bridge_encryption_mode IN ('off', 'required')),
    relay_ca_ciphertext BLOB,
    relay_ca_nonce BLOB,
    relay_ca_key_version INTEGER,
    client_cert_ciphertext BLOB,
    client_cert_nonce BLOB,
    client_cert_key_version INTEGER,
    client_key_ciphertext BLOB,
    client_key_nonce BLOB,
    client_key_key_version INTEGER,
    bridge_encryption_key_ciphertext BLOB,
    bridge_encryption_key_nonce BLOB,
    bridge_encryption_key_key_version INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name),
    CHECK ((relay_ca_ciphertext IS NULL) = (relay_ca_nonce IS NULL)
        AND (relay_ca_ciphertext IS NULL) = (relay_ca_key_version IS NULL)),
    CHECK ((client_cert_ciphertext IS NULL) = (client_cert_nonce IS NULL)
        AND (client_cert_ciphertext IS NULL) = (client_cert_key_version IS NULL)),
    CHECK ((client_key_ciphertext IS NULL) = (client_key_nonce IS NULL)
        AND (client_key_ciphertext IS NULL) = (client_key_key_version IS NULL)),
    CHECK ((bridge_encryption_key_ciphertext IS NULL) = (bridge_encryption_key_nonce IS NULL)
        AND (bridge_encryption_key_ciphertext IS NULL) = (bridge_encryption_key_key_version IS NULL))
);

CREATE TABLE IF NOT EXISTS standalone_provider_endpoints (
    endpoint_id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    provider TEXT NOT NULL CHECK (provider IN ('generic', 'minimax')),
    provider_region TEXT CHECK (provider_region IS NULL OR provider_region IN ('cn', 'global')),
    base_url TEXT NOT NULL,
    native_api TEXT NOT NULL CHECK (native_api IN ('auto', 'anthropic_messages', 'chat', 'responses', 'realtime')),
    native_api_source TEXT NOT NULL CHECK (native_api_source IN ('auto', 'detected', 'manual')),
    key_lb_enabled INTEGER NOT NULL CHECK (key_lb_enabled IN (0, 1)),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    mcp_enabled INTEGER NOT NULL CHECK (mcp_enabled IN (0, 1)),
    api_key_ciphertext BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    api_key_key_version INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS standalone_endpoint_keys (
    key_id TEXT PRIMARY KEY,
    endpoint_id TEXT NOT NULL REFERENCES standalone_provider_endpoints(endpoint_id) ON DELETE CASCADE,
    key_label TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    position INTEGER NOT NULL,
    api_key_ciphertext BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    api_key_key_version INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(endpoint_id, key_label),
    UNIQUE(endpoint_id, position)
);

CREATE TABLE IF NOT EXISTS standalone_model_routes (
    rule_id TEXT PRIMARY KEY,
    scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
    owner_user_id INTEGER,
    model_pattern TEXT NOT NULL,
    routing_strategy TEXT NOT NULL CHECK (routing_strategy IN ('client_key_rendezvous', 'responses_session_affinity')),
    daily_max_requests INTEGER,
    monthly_max_requests INTEGER,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((scope = 'admin' AND owner_user_id IS NULL) OR (scope = 'user' AND owner_user_id IS NOT NULL)),
    UNIQUE(scope, owner_user_id, model_pattern)
);

CREATE TABLE IF NOT EXISTS standalone_model_route_targets (
    target_id TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL REFERENCES standalone_model_routes(rule_id) ON DELETE CASCADE,
    endpoint_id TEXT NOT NULL REFERENCES standalone_provider_endpoints(endpoint_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    upstream_model TEXT,
    responses_continuation_policy TEXT NOT NULL CHECK (responses_continuation_policy IN ('force_passthrough', 'force_replay')),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(rule_id, position)
);

CREATE TABLE IF NOT EXISTS standalone_client_keys (
    key_id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL,
    key_prefix TEXT NOT NULL,
    label TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    secret_ciphertext BLOB NOT NULL,
    secret_nonce BLOB NOT NULL,
    secret_key_version INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS standalone_settings (
    setting_key TEXT PRIMARY KEY,
    value_version INTEGER NOT NULL CHECK (value_version > 0),
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_standalone_endpoint_keys_endpoint
    ON standalone_endpoint_keys(endpoint_id, enabled, position);
CREATE INDEX IF NOT EXISTS idx_standalone_routes_enabled
    ON standalone_model_routes(scope, owner_user_id, enabled);
CREATE INDEX IF NOT EXISTS idx_standalone_route_targets_rule
    ON standalone_model_route_targets(rule_id, enabled, position);
