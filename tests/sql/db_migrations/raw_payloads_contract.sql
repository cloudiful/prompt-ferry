-- All probes are scoped to the current schema because the shared test database
-- may contain leftover schemas from other test runs.
SELECT
    EXISTS(
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name = 'request_raw_json'
    ) AS old_request_raw_column,
    EXISTS(
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name = 'response_raw_body'
    ) AS old_response_raw_column,
    EXISTS(
        SELECT 1
        FROM pg_class
        WHERE relnamespace = current_schema()::regnamespace
          AND relname = 'request_record_raw_payloads'
          AND relkind = 'p'
    ) AS raw_parent_is_partitioned,
    EXISTS(
        SELECT 1
        FROM pg_inherits inheritance
        JOIN pg_class child ON child.oid = inheritance.inhrelid
        WHERE child.relnamespace = current_schema()::regnamespace
          AND child.relname = 'request_record_raw_payloads_default'
    ) AS default_partition_exists,
    EXISTS(
        SELECT 1
        FROM pg_class
        WHERE relnamespace = current_schema()::regnamespace
          AND relname = 'request_record_raw_payloads_overflow'
          AND relkind = 'r'
    )
    AND NOT EXISTS(
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_record_raw_payloads_overflow'
          AND column_name IN ('request_raw_json', 'response_raw_body')
    ) AS overflow_table_exists,
    EXISTS(
        SELECT 1
        FROM pg_indexes
        WHERE schemaname = current_schema()
          AND tablename = 'request_record_raw_payloads'
          AND indexname = 'idx_request_record_raw_payloads_event_id'
    ) AS event_id_index_exists,
    EXISTS(
        SELECT 1
        FROM pg_indexes
        WHERE schemaname = current_schema()
          AND tablename = 'request_record_raw_payloads'
          AND indexname = 'idx_request_record_raw_payloads_created_at_event_id'
    ) AS created_at_event_id_index_exists;
