ALTER TABLE model_route_targets DROP CONSTRAINT IF EXISTS ck_model_route_targets_native_api;
ALTER TABLE model_route_targets DROP COLUMN IF EXISTS native_api;
