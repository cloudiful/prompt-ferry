-- P6 (issue #570): drop the obsolete auto-named inline CHECK that 0001
-- created on `provider_endpoints.native_api` (`responses`/`chat` only).
-- The canonical `ck_provider_endpoints_native_api`
-- (`auto`/`responses`/`chat`/`anthropic_messages`, see 0052) remains in
-- place and keeps rejecting unknown values, so this only removes the stale
-- narrower duplicate that wrongly rejects `anthropic_messages`. No rows are
-- touched.
ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS provider_endpoints_native_api_check;
