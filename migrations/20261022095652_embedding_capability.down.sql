-- Issue #1032 P0: revert the first-class embeddings capability.
--
-- The dimension column goes away with its CHECK constraint. Restoring the
-- narrower `native_api` CHECKs fails the transaction when a row already uses
-- `embeddings`: the rollback is fail-closed and never rewrites or deletes a
-- row to make the previous constraint fit.
ALTER TABLE model_route_targets
DROP COLUMN IF EXISTS embedding_dimensions_override;

ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_native_api;

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_native_api
CHECK (native_api IN ('auto', 'responses', 'chat', 'anthropic_messages', 'realtime'));

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api
CHECK (native_api IN ('auto', 'responses', 'chat', 'anthropic_messages'));
