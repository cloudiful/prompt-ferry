-- Issue #466: remove client_key_rendezvous, unify multi-key routing on
-- responses_session_affinity. SQLite cannot ALTER a CHECK, so the parent
-- table is rebuilt with the narrowed CHECK. Data migrates first so the
-- rebuild only ever sees the single remaining value.
UPDATE standalone_model_routes
SET routing_strategy = 'responses_session_affinity'
WHERE routing_strategy = 'client_key_rendezvous';

PRAGMA foreign_keys=OFF;

CREATE TABLE standalone_model_routes_new (
    rule_id TEXT PRIMARY KEY,
    scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
    owner_user_id INTEGER,
    model_pattern TEXT NOT NULL,
    routing_strategy TEXT NOT NULL CHECK (routing_strategy IN ('responses_session_affinity')),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((scope = 'admin' AND owner_user_id IS NULL) OR (scope = 'user' AND owner_user_id IS NOT NULL)),
    UNIQUE(scope, owner_user_id, model_pattern)
);

INSERT INTO standalone_model_routes_new (
    rule_id, scope, owner_user_id, model_pattern, routing_strategy, enabled, created_at, updated_at
) SELECT
    rule_id, scope, owner_user_id, model_pattern, routing_strategy, enabled, created_at, updated_at
FROM standalone_model_routes;

DROP TABLE standalone_model_routes;

ALTER TABLE standalone_model_routes_new RENAME TO standalone_model_routes;

CREATE INDEX IF NOT EXISTS idx_standalone_routes_enabled
    ON standalone_model_routes(scope, owner_user_id, enabled);

PRAGMA foreign_keys=ON;

UPDATE standalone_schema_meta
SET schema_version = 27
WHERE schema_key = 'standalone';
