-- Destructive retention migration: existing PostgreSQL raw request/response
-- bodies are intentionally deleted. Raw payloads are stored only in the
-- configured object store (or the local filesystem fallback); PostgreSQL keeps
-- non-body object metadata (key, size, hash, expiry) for Admin lookup and
-- partitioned retention.
ALTER TABLE request_record_raw_payloads
    DROP COLUMN IF EXISTS request_raw_json,
    DROP COLUMN IF EXISTS response_raw_body;

ALTER TABLE request_record_raw_payloads_overflow
    DROP COLUMN IF EXISTS request_raw_json,
    DROP COLUMN IF EXISTS response_raw_body;
