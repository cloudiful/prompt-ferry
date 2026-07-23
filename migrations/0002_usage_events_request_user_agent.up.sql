ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS request_user_agent TEXT;
