SELECT
    data_type::TEXT AS "data_type!",
    (is_nullable = 'NO') AS "is_not_null!",
    (column_default IS NOT NULL) AS "has_default!",
    EXISTS (
        SELECT 1
        FROM pg_constraint c
        JOIN pg_class t ON t.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE n.nspname = current_schema()
          AND t.relname = 'request_records'
          AND c.conname = 'ck_usage_event_request_storage_mode'
    ) AS "has_storage_constraint!"
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'request_records'
  AND column_name = 'request_storage_mode'
