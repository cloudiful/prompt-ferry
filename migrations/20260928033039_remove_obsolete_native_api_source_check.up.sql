-- P6 (issue #570): drop the obsolete auto-named inline CHECK that 0001
-- created on `provider_endpoints.native_api_source`
-- (`detected`/`manual` only). The canonical
-- `ck_provider_endpoints_native_api_source`
-- (`auto`/`detected`/`manual`, see 0052) remains in place and keeps
-- rejecting unknown values, so this only removes the stale narrower
-- duplicate that wrongly rejects `auto` (reached via Auto protocol mode).
-- No rows are touched.
ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS provider_endpoints_native_api_source_check;
