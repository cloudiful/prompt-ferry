-- Issue #368 Phase A: mirror PostgreSQL 0081 outbound proxy columns for
-- the standalone SQLite store. Secrets use the envelope pattern
-- (`*_ciphertext BLOB / *_nonce BLOB / *_key_version INTEGER`, NULL means
-- direct), matching `api_key` and the relay secret columns. No CHECK,
-- matching the pre-existing standalone convention (allowed set stays
-- enforced by shared admin validation in Phase B).
ALTER TABLE standalone_provider_endpoints ADD COLUMN proxy_url_ciphertext BLOB;
ALTER TABLE standalone_provider_endpoints ADD COLUMN proxy_url_nonce BLOB;
ALTER TABLE standalone_provider_endpoints ADD COLUMN proxy_url_key_version INTEGER;

ALTER TABLE standalone_model_route_targets ADD COLUMN proxy_url_override_ciphertext BLOB;
ALTER TABLE standalone_model_route_targets ADD COLUMN proxy_url_override_nonce BLOB;
ALTER TABLE standalone_model_route_targets ADD COLUMN proxy_url_override_key_version INTEGER;

UPDATE standalone_schema_meta
SET schema_version = 18
WHERE schema_key = 'standalone';
