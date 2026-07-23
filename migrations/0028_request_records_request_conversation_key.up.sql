ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS request_conversation_key TEXT;
