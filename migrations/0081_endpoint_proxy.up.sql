-- Issue #368 Phase A: outbound proxy defaults (endpoint) and per-target
-- overrides (route). `proxy_url` carries the full proxy URL including
-- optional userinfo (`http://user:pass@host:port`); NULL means direct.
-- Plaintext on PostgreSQL mirrors the existing `api_key` asymmetry
-- (SQLite uses the envelope columns in standalone 0018). Scheme
-- validation lives in the admin layer (Phase B); no CHECK here so
-- legacy rows are never broken.
ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS proxy_url TEXT;

ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS proxy_url_override TEXT;
