-- Issue #589: optional per-endpoint OpenAI Admin API key used by the official
-- organization usage/cost read. Mirrors `api_key`/`proxy_url`: plaintext in
-- PostgreSQL, never echoed by an admin response (the response only exposes
-- whether one is stored via `has_admin_api_key`).
ALTER TABLE provider_endpoints
    ADD COLUMN admin_api_key TEXT;
