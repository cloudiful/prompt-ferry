-- Issue #502 Task 5: per-target compact mode. Defaults to `passthrough`
-- (no new default semantics); `self_summarize` enables ferry-side handoff
-- summarization for non-Responses targets; `off` rejects compact explicitly.
ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS compact_mode TEXT NOT NULL DEFAULT 'passthrough';

ALTER TABLE model_route_targets
DROP CONSTRAINT IF EXISTS ck_model_route_targets_compact_mode;

ALTER TABLE model_route_targets
ADD CONSTRAINT ck_model_route_targets_compact_mode
CHECK (compact_mode IN ('passthrough', 'self_summarize', 'off'));
