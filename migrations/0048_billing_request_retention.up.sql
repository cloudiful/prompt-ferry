ALTER TABLE usage_charges
    DROP CONSTRAINT IF EXISTS usage_charges_event_id_fkey;

ALTER TABLE usage_charges
    ALTER COLUMN event_id DROP NOT NULL;

ALTER TABLE usage_charges
    ADD CONSTRAINT usage_charges_event_id_fkey
    FOREIGN KEY (event_id) REFERENCES request_records(event_id) ON DELETE SET NULL;
