SELECT
    EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name = 'ttft_ms'
    ) AS "ttft_exists!",
    NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name = 'first_chunk_ms'
    ) AS "first_chunk_absent!"
