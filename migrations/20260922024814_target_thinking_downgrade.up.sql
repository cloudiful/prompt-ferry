-- Issue #566: per-target thinking adaptation switch.
-- `thinking_downgrade_enabled` gates the #556 pre-flight thinking downgrade
-- and the reasoning-echo fingerprint retry on a target. Default FALSE keeps
-- the pre-#556 byte-identical passthrough; frontend always sends true/false
-- (no omit semantics). The `PROMPT_FERRY_DISABLE_THINKING_DOWNGRADE=1` escape
-- hatch still forces a full bypass. No CHECK beyond NOT NULL DEFAULT so
-- legacy rows read as disabled.
ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS thinking_downgrade_enabled BOOLEAN NOT NULL DEFAULT FALSE;
