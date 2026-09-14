-- Issue #375 Phase F: per-row MCP outbound proxy. `proxy_url` carries the
-- full proxy URL including optional userinfo
-- (`http://user:pass@host:port`); NULL means inherit (row empty falls back
-- to process env, then direct). Plaintext on PostgreSQL mirrors the
-- endpoint `proxy_url` asymmetry (SQLite uses the envelope columns in
-- standalone 0019). Scheme validation lives in the admin layer (reuse #368
-- whitelist http/https/socks5/socks5h); no CHECK here so legacy rows are
-- never broken.
ALTER TABLE mcp_servers
ADD COLUMN IF NOT EXISTS proxy_url TEXT;
