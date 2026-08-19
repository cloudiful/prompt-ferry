SELECT NOT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'model_route_targets'
      AND column_name = 'chat_reasoning_replay_policy'
) AS removed;
