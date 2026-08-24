-- Phase 1C-a: extend the standalone SQLite request ledger with the non-secret
-- request metadata fields that are carried by `RequestRecordCreate` but are
-- dropped by the compact table introduced in Phase 1A. The migration only
-- adds non-body metadata: raw request/response bodies, encrypted upstream
-- redaction sessions, billing snapshots, approvals, and quota state remain
-- out of scope for this slice and will arrive in later subphases of
-- Phase 1C.
--
-- Boolean columns keep the `INTEGER NOT NULL DEFAULT 0 CHECK (... IN (0,1))`
-- pattern used by the rest of the schema so existing row-level corruption
-- handling in `parse_usage_summary_row` continues to validate them as
-- 0/1 without changing the read path. Optional identifier, label, and
-- text columns are nullable so `ALTER TABLE ADD COLUMN` works on existing
-- pre-1C-a rows without backfill, and the corresponding Rust DTOs
-- (`Option<...>`) round-trip through `NULL` cleanly.

ALTER TABLE standalone_usage_summaries
    ADD COLUMN user_id INTEGER;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN client_key_id INTEGER;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN client_key_label TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN request_user_agent TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN endpoint_key_label TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN mcp_server_name TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN mcp_protocol_method TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN mcp_operation_name TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN http_request_content_encoding TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN http_request_compressed INTEGER NOT NULL DEFAULT 0
    CHECK (http_request_compressed IN (0, 1));
ALTER TABLE standalone_usage_summaries
    ADD COLUMN http_request_compressed_bytes INTEGER;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN http_request_decompressed_bytes INTEGER;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN http_request_compression_ratio REAL;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN conversation_source TEXT NOT NULL DEFAULT 'none';
ALTER TABLE standalone_usage_summaries
    ADD COLUMN client_installation_id TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN provider_response_id TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN provider_conversation_key TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN request_storage_mode TEXT NOT NULL DEFAULT 'full';
ALTER TABLE standalone_usage_summaries
    ADD COLUMN error_message TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN request_has_previous_response_id INTEGER NOT NULL DEFAULT 0
    CHECK (request_has_previous_response_id IN (0, 1));
ALTER TABLE standalone_usage_summaries
    ADD COLUMN request_previous_response_id TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN request_previous_response_parent_found INTEGER;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN request_conversation_key TEXT;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN request_conversation_parent_found INTEGER;
ALTER TABLE standalone_usage_summaries
    ADD COLUMN upstream_redaction_enabled INTEGER NOT NULL DEFAULT 0
    CHECK (upstream_redaction_enabled IN (0, 1));
ALTER TABLE standalone_usage_summaries
    ADD COLUMN response_capture_truncated INTEGER NOT NULL DEFAULT 0
    CHECK (response_capture_truncated IN (0, 1));

UPDATE standalone_schema_meta
SET schema_version = 7
WHERE schema_key = 'standalone';