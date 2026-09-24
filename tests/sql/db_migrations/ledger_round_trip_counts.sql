-- Round-trip probe for the partition family down migration: every table that
-- carries a surviving FK into `request_records` is filled before `down`, and
-- `down` must stay executable by truncating all of them before it restores
-- the legacy foreign keys.
SELECT
    (SELECT count(*) FROM usage_charges) AS "charges!",
    (SELECT count(*) FROM usage_charge_lines) AS "charge_lines!",
    (SELECT count(*) FROM conversation_redaction_sessions) AS "redaction_sessions!",
    (SELECT count(*) FROM request_record_raw_payloads) AS "raw_payloads!",
    (SELECT count(*) FROM request_record_raw_payloads_overflow) AS "raw_payload_overflow!"
