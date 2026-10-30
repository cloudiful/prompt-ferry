-- Issue #1132 P1: reverse the per-channel cache-alert cooldown state.
--
-- The down path refuses to run while SMTP cooldown rows exist: dropping the
-- channel would silently discard the SMTP cooldown history that the up
-- migration made possible, and a migration must never drop business rows
-- quietly. Only a database that never recorded an SMTP alert is restored to
-- the original single-column primary key.
DO $$
DECLARE
    smtp_rows BIGINT;
BEGIN
    IF EXISTS (
        SELECT 1
        FROM pg_attribute
        WHERE attrelid = to_regclass('cache_alert_state')
          AND attname = 'channel'
          AND attnum > 0
          AND NOT attisdropped
    ) THEN
        SELECT count(*)
        INTO smtp_rows
        FROM cache_alert_state
        WHERE channel = 'smtp';

        IF smtp_rows > 0 THEN
            RAISE EXCEPTION
                'cannot revert cache_alert_state channel state: % SMTP cooldown rows exist, refusing to discard them',
                smtp_rows;
        END IF;
    END IF;
END $$;

ALTER TABLE cache_alert_state
    DROP CONSTRAINT IF EXISTS ck_cache_alert_state_channel;

ALTER TABLE cache_alert_state
    DROP CONSTRAINT IF EXISTS cache_alert_state_pkey;

ALTER TABLE cache_alert_state
    ADD CONSTRAINT cache_alert_state_pkey
    PRIMARY KEY (conversation_id);

ALTER TABLE cache_alert_state
    DROP COLUMN IF EXISTS channel;
