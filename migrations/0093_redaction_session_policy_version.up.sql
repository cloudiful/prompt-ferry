-- Issue #524 Task 5: stamp upstream redaction sessions with the policy
-- generation that produced their tokens. A row whose `policy_version` does not
-- match the running generation is ignored on read and replaced on write, so a
-- mid-conversation redaction toggle never reuses the previous token counter.
ALTER TABLE conversation_redaction_sessions
ADD COLUMN IF NOT EXISTS policy_version BIGINT NOT NULL DEFAULT 0;
