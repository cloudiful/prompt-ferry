-- Issue #1132 P1: per-channel cache-alert cooldown state.
--
-- `cache_alert_state` holds one cooldown row per conversation, which only
-- models the DingTalk channel. SMTP needs its own cooldown clock for the
-- same conversation, so the row key becomes (conversation_id, channel) and
-- every existing row keeps the DingTalk channel it already represented.
--
-- The channel is constrained to the two known channels so a mistyped channel
-- cannot create a cooldown row that no channel ever reads, and the column
-- default keeps the legacy single-channel insert path working.
--
-- Every statement is safe against a database that already carries this exact
-- structure, so the same shape can be pre-applied by hand and still be
-- applied by the migration framework afterwards.
ALTER TABLE cache_alert_state
    ADD COLUMN IF NOT EXISTS channel TEXT NOT NULL DEFAULT 'dingtalk';

ALTER TABLE cache_alert_state
    ALTER COLUMN channel SET DEFAULT 'dingtalk';

ALTER TABLE cache_alert_state
    ALTER COLUMN channel SET NOT NULL;

ALTER TABLE cache_alert_state
    DROP CONSTRAINT IF EXISTS ck_cache_alert_state_channel;

ALTER TABLE cache_alert_state
    ADD CONSTRAINT ck_cache_alert_state_channel
    CHECK (channel IN ('dingtalk', 'smtp'));

ALTER TABLE cache_alert_state
    DROP CONSTRAINT IF EXISTS cache_alert_state_pkey;

ALTER TABLE cache_alert_state
    ADD CONSTRAINT cache_alert_state_pkey
    PRIMARY KEY (conversation_id, channel);
