SELECT
    EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'request_records'
          AND column_name = 'content_expired_at'
    ) AS "content_expired_at_exists!",
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
    ) AS "legacy_payload_columns_removed!"
