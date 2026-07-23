ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS upstream_model TEXT;
