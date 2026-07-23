ALTER TABLE model_route_targets
ADD COLUMN responses_passthrough BOOLEAN;

UPDATE model_route_targets
SET responses_passthrough = CASE
    WHEN responses_continuation_policy = 'force_passthrough' THEN TRUE
    ELSE FALSE
END
WHERE responses_passthrough IS NULL;

ALTER TABLE model_route_targets
ALTER COLUMN responses_passthrough SET NOT NULL;

ALTER TABLE model_route_targets
ALTER COLUMN responses_passthrough SET DEFAULT FALSE;

ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_responses_continuation_policy;

ALTER TABLE model_route_targets
DROP COLUMN responses_continuation_policy;
