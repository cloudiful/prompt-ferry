ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS response_raw_body TEXT;
