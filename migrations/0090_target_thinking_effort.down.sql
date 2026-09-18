ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_thinking_effort;

ALTER TABLE model_route_targets
DROP COLUMN IF EXISTS thinking_effort_override;
