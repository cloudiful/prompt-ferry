-- Per-target thinking effort override. NULL/empty means inherit (follow caller).
ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS thinking_effort_override TEXT NULL;

ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_thinking_effort;

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_thinking_effort
CHECK (thinking_effort_override IS NULL OR thinking_effort_override IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'));
