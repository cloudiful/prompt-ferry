-- Issue #1032 P0: first-class embeddings capability.
--
-- `native_api` gains the `embeddings` protocol at the endpoint and the
-- model-route target level, so an operator can point a target at an
-- OpenAI-compatible embeddings backend explicitly. Every previously allowed
-- value stays allowed; no row is rewritten and no default changes.
--
-- `model_route_targets.embedding_dimensions_override` is a nullable positive
-- integer: NULL inherits the caller's dimensions, a value forces the
-- dimension count for that target. It is an override, never a cap, so the
-- only schema rule is "positive integer" (PostgreSQL `INTEGER` is already
-- capped at the i32 maximum).
ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api
CHECK (native_api IN ('auto', 'responses', 'chat', 'anthropic_messages', 'embeddings'));

ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_native_api;

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_native_api
CHECK (native_api IN ('auto', 'responses', 'chat', 'anthropic_messages', 'realtime', 'embeddings'));

ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS embedding_dimensions_override INTEGER;

ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_embedding_dimensions_override;

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_embedding_dimensions_override
CHECK (embedding_dimensions_override IS NULL OR embedding_dimensions_override > 0);
