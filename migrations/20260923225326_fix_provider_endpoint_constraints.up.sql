-- Issue #584: two provider_endpoints constraint defects survive every later
-- widening and only surface on a database built by 0001.
--
-- 1. 0001 declares the protocol columns inline
--    (`native_api TEXT ... CHECK (...)`), so PostgreSQL creates the auto-named
--    `provider_endpoints_native_api_check` / `..._source_check`. 0039/0052 only
--    manage the explicitly named `ck_provider_endpoints_native_api(_source)`
--    pair, so the auto-named backstop keeps rejecting every kind added later
--    (`anthropic_messages`, `realtime`, and the `auto` source). Drop the
--    auto-named constraints and pin the named pair to the full kind set the
--    admin handler can persist.
ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS provider_endpoints_native_api_check;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS provider_endpoints_native_api_source_check;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api
CHECK (native_api IN ('auto', 'anthropic_messages', 'chat', 'responses', 'realtime'));

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api_source;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api_source
CHECK (native_api_source IN ('auto', 'detected', 'manual'));

-- 2. `provider_region IN ('cn', 'global')` evaluates to NULL when the region is
--    NULL and a CHECK only rejects FALSE, so the 0062/0079 shape let MiniMax
--    rows land without a region. Spell the region domain out explicitly and
--    require the MiniMax branch to carry a concrete region; the same NULL-safe
--    domain is what the standalone schema pins.
ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_provider_region;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_provider_region
CHECK (
    (provider_region IS NULL OR provider_region IN ('cn', 'global'))
    AND (
        (provider = 'minimax' AND provider_region IS NOT NULL)
        OR (provider <> 'minimax' AND provider_region IS NULL)
    )
);
