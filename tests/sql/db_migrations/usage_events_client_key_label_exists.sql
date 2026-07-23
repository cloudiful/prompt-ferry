SELECT EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'request_records'
      AND column_name = 'client_key_label'
) AS "exists!"
