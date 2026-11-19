-- Raw bodies return to PostgreSQL. A destination that already holds raw
-- payload history cannot be converted here: the previous object-store objects
-- are not reachable from SQL, so this migration refuses to run instead of
-- silently discarding rows. The preflight and the schema change share the
-- migration transaction, so a non-empty destination is left untouched.
DO $$
DECLARE
    parent_rows BIGINT;
    overflow_rows BIGINT;
BEGIN
    SELECT count(*) INTO parent_rows FROM request_record_raw_payloads;
    SELECT count(*) INTO overflow_rows FROM request_record_raw_payloads_overflow;
    IF parent_rows <> 0 OR overflow_rows <> 0 THEN
        RAISE EXCEPTION
            'request_record_raw_payloads is not empty (% parent row(s), % overflow row(s)): a fresh destination is required before raw bodies can be stored in PostgreSQL',
            parent_rows, overflow_rows;
    END IF;
END
$$;

ALTER TABLE request_record_raw_payloads
    ADD COLUMN request_raw_json JSONB,
    ADD COLUMN response_raw_body TEXT;

ALTER TABLE request_record_raw_payloads
    ADD CONSTRAINT ck_request_record_raw_payloads_has_payload CHECK (
        request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL
    );

ALTER TABLE request_record_raw_payloads_overflow
    ADD COLUMN request_raw_json JSONB,
    ADD COLUMN response_raw_body TEXT;

ALTER TABLE request_record_raw_payloads_overflow
    ADD CONSTRAINT ck_request_record_raw_payloads_overflow_has_payload CHECK (
        request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL
    );
