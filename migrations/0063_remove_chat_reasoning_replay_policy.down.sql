ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS chat_reasoning_replay_policy TEXT;

UPDATE model_route_targets
SET chat_reasoning_replay_policy = 'auto'
WHERE chat_reasoning_replay_policy IS NULL;

ALTER TABLE model_route_targets
ALTER COLUMN chat_reasoning_replay_policy SET NOT NULL;

ALTER TABLE model_route_targets
ALTER COLUMN chat_reasoning_replay_policy SET DEFAULT 'auto';

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_chat_reasoning_replay_policy
CHECK (chat_reasoning_replay_policy IN ('auto', 'force_replay', 'force_passthrough'));
