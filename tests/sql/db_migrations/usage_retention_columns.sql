SELECT
    NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name = 'content_expired_at'
    ) AS "content_expired_at_removed!",
    EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name IN ('request_full_json', 'request_delta_json', 'response_prompt', 'upstream_error_body')
    ) AS "content_columns_in_metadata!",
    EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_record_content'
          AND column_name IN ('request_full_json', 'request_delta_json', 'response_prompt', 'upstream_error_body')
    ) AS "content_table_exists!",
    EXISTS (
        SELECT 1
        FROM pg_class
        WHERE relnamespace = current_schema()::regnamespace
          AND relname = 'request_records'
          AND relkind = 'p'
    ) AS "request_records_partitioned!",
    EXISTS (
        SELECT 1
        FROM pg_class
        WHERE relnamespace = current_schema()::regnamespace
          AND relname = 'request_record_content'
          AND relkind = 'p'
    ) AS "content_table_partitioned!",
    EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_record_raw_payloads'
          AND column_name = 'raw_object_key'
    ) AS "raw_object_key_exists!",
    NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name IN (
              'request_prompt',
              'request_full_text',
              'request_delta_text',
              'upstream_redacted_request_json',
              'restore_session_ciphertext',
              'restore_session_nonce',
              'restore_session_key_version'
          )
    ) AS "legacy_payload_columns_removed!",
    NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name IN ('request_record_raw_payloads', 'request_record_raw_payloads_overflow')
          AND column_name IN ('request_raw_json', 'response_raw_body')
    ) AS "raw_body_columns_removed!"
-- All probes are scoped to the current schema because the shared test database
-- may contain leftover schemas from other test runs.
