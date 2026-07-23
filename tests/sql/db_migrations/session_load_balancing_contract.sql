SELECT
    EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'model_endpoint_rules'
          AND column_name = 'session_affinity_lock_after_turns'
          AND is_nullable = 'NO'
          AND column_default = '5'
    ) AS "lock_turns_column_exists!",
    EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'request_records'::regclass
          AND conname = 'ck_request_records_route_selection_reason'
          AND pg_get_constraintdef(oid) LIKE '%session_load_balance%'
    ) AS "route_reason_exists!"
