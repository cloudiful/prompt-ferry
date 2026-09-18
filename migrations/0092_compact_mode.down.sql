ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_compact_mode;

ALTER TABLE model_route_targets
DROP COLUMN IF EXISTS compact_mode;
