-- Issue #378 Phase I: per-target effective time windows. `active_windows`
-- carries a JSON array of `{start,end}` HH:MM pairs (e.g.
-- `[{"start":"06:30","end":"14:00"}]`); NULL/empty means all-day.
-- `end < start` is overnight (e.g. 22:00-06:00); overlaps allowed.
-- Validation lives in the admin layer; no CHECK here so legacy rows
-- are never broken.
ALTER TABLE model_route_targets
ADD COLUMN IF NOT EXISTS active_windows TEXT;
