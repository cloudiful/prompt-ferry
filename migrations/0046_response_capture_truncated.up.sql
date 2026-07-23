ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS response_capture_truncated BOOLEAN NOT NULL DEFAULT FALSE;
