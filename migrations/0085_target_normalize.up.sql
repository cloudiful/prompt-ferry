-- Issue #392 Phase K: per-target developer->system normalization switch.
-- `dev_system_normalize` controls whether Chat->Chat passthrough rewrites
-- `developer` roles to `system` for strict OpenAI-compatible upstreams.
-- Default FALSE (passthrough unchanged); frontend always sends true/false
-- (no omit semantics). No CHECK beyond NOT NULL DEFAULT so legacy rows
-- read as disabled.
ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS dev_system_normalize BOOLEAN NOT NULL DEFAULT FALSE;
