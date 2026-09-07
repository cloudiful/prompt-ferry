SELECT NOT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'model_route_targets'
      AND column_name = 'responses_continuation_policy'
) AS removed,
NOT EXISTS (
    SELECT 1
    FROM pg_constraint c
    JOIN pg_class t ON t.oid = c.conrelid
    JOIN pg_namespace n ON n.oid = t.relnamespace
    WHERE n.nspname = current_schema()
      AND t.relname = 'model_route_targets'
      AND c.conname = 'ck_model_route_targets_responses_continuation_policy'
) AS constraint_removed;
