SELECT
  COUNT(*)::BIGINT AS "total!",
  COUNT(*) FILTER (WHERE enabled)::BIGINT AS "enabled_count!"
FROM managed_relays;
