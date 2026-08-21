SELECT COUNT(*) AS total,
       SUM(CASE WHEN enabled = 1 THEN 1 ELSE 0 END) AS enabled_count
FROM standalone_relays;