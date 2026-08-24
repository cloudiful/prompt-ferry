-- Compact standalone SQLite request ledger. The schema mirrors the fields
-- carried by `StandaloneUsageSummary` and intentionally excludes raw request
-- and response bodies, billing fields, and approval/quota state. Those are
-- scheduled for later subphases of Phase 1.
--
-- `event_id` is the independent insertion key so a single request can
-- record every lifecycle event (Received, Completed, Failed, Aborted) as a
-- distinct row instead of collapsing every later event into the first one.

CREATE TABLE IF NOT EXISTS standalone_usage_summaries (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    category TEXT NOT NULL,
    state TEXT NOT NULL,
    path TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    status INTEGER,
    ok INTEGER,
    duration_ms INTEGER,
    ttft_ms INTEGER,
    model TEXT,
    requested_model TEXT,
    upstream_model TEXT,
    endpoint_id TEXT,
    endpoint_key_id TEXT,
    model_route_rule_id TEXT,
    mcp_server_id TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    total_tokens INTEGER,
    cached_tokens INTEGER,
    cache_read_tokens INTEGER,
    cache_write_tokens INTEGER,
    error_code TEXT,
    failure_family TEXT,
    redaction_applied INTEGER NOT NULL CHECK (redaction_applied IN (0, 1)),
    redaction_findings_count INTEGER NOT NULL,
    redaction_replacements_count INTEGER NOT NULL,
    redaction_types_json TEXT NOT NULL DEFAULT '[]',
    redaction_fields_json TEXT NOT NULL DEFAULT '[]',
    route_selection_reason TEXT NOT NULL,
    CHECK (ok IS NULL OR ok IN (0, 1)),
    CHECK (status IS NULL OR (status >= 100 AND status < 1000))
);

CREATE INDEX IF NOT EXISTS idx_standalone_usage_summaries_recorded_at
    ON standalone_usage_summaries(recorded_at);
CREATE INDEX IF NOT EXISTS idx_standalone_usage_summaries_request_id
    ON standalone_usage_summaries(request_id);

UPDATE standalone_schema_meta
SET schema_version = 6
WHERE schema_key = 'standalone';
