ALTER TABLE model_route_targets
ADD COLUMN responses_continuation_policy TEXT;

UPDATE model_route_targets
SET responses_continuation_policy = CASE
    WHEN responses_passthrough THEN 'force_passthrough'
    ELSE 'force_replay'
END
WHERE responses_continuation_policy IS NULL;

ALTER TABLE model_route_targets
ALTER COLUMN responses_continuation_policy SET NOT NULL;

ALTER TABLE model_route_targets
ALTER COLUMN responses_continuation_policy SET DEFAULT 'force_replay';

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_responses_continuation_policy
CHECK (responses_continuation_policy IN ('force_passthrough', 'force_replay'));

ALTER TABLE model_route_targets
DROP COLUMN responses_passthrough;
