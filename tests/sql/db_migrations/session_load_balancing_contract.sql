SELECT
    EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'request_records'::regclass
          AND conname = 'ck_request_records_route_selection_reason'
          AND pg_get_constraintdef(oid) LIKE '%session_load_balance%'
    ) AS "route_reason_exists!"
