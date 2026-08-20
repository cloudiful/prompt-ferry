-- Returns the charge_id for one `usage_charges` row keyed by event_id,
-- or NULL when no charge exists yet. Used by the backfill integration
-- tests to look up the charge that `record_usage_charge` auto-creates.
SELECT charge_id
FROM usage_charges
WHERE event_id = $1
