-- Issue #409 Phase 1: per-target native API override. `native_api` carries
-- the target-level port type (`auto` default = follow the caller, resolved
-- via the existing `resolve_auto_protocol` mapping). An explicit value wins
-- over the upstream endpoint `native_api`; `auto` falls back to the endpoint
-- then global `config.upstream_native_api`. No backfill beyond the DEFAULT
-- so legacy rows read as `auto`.
ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS native_api TEXT NOT NULL DEFAULT 'auto';

ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_native_api;

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_native_api
CHECK (native_api IN ('auto', 'responses', 'chat', 'anthropic_messages', 'realtime'));
