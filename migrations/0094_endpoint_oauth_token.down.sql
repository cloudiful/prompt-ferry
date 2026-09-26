-- Reverse 0094: drop the per-endpoint ChatGPT OAuth token table (issue
-- #599 R2a). The table has no dependents, so a plain drop is sufficient.
DROP TABLE IF EXISTS endpoint_oauth_tokens;
