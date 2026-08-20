-- Reads the current pricing_status for one event's `usage_charges` row, if
-- it exists. Used by the historical backfill to detect a "priced" charge
-- that no longer has a matching `billing_price_rules` entry. In that case
-- the backfill must surface a `Failed` outcome instead of silently dropping
-- the charge to `unpriced`. A missing row returns NULL.
SELECT pricing_status AS "pricing_status?"
FROM usage_charges
WHERE event_id = $1
