SELECT
    EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'usage_charges_event_id_fkey'
          AND conrelid = 'usage_charges'::regclass
    ) AS "charges_fkey!",
    EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'conversation_redaction_sessions_last_event_id_fkey'
          AND conrelid = 'conversation_redaction_sessions'::regclass
    ) AS "redaction_fkey!",
    EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'request_record_raw_payloads_event_id_fkey'
          AND conrelid = 'request_record_raw_payloads'::regclass
    ) AS "raw_payloads_fkey!",
    EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'request_record_raw_payloads_overflow_event_id_fkey'
          AND conrelid = 'request_record_raw_payloads_overflow'::regclass
    ) AS "raw_payloads_overflow_fkey!",
    EXISTS (
        SELECT 1
        FROM pg_class
        WHERE relname = 'request_records'
          AND relkind = 'r'
    ) AS "records_plain!"
