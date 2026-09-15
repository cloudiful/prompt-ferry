-- Issue #392 Phase K: per-endpoint effective time windows. `active_windows`
-- carries a JSON array of `{start,end}` HH:MM pairs (e.g.
-- `[{"start":"06:30","end":"14:00"}]`); NULL/empty means all-day.
-- `end < start` is overnight (e.g. 22:00-06:00); overlaps allowed.
-- Effective windows resolve as target-nonempty else endpoint else all-day
-- (target empty inherits the endpoint). Validation lives in the admin
-- layer (same HH:MM helper as targets); no CHECK here so legacy rows
-- are never broken.
ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS active_windows TEXT;
