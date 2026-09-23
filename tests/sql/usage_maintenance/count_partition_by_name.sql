SELECT COUNT(*)::BIGINT AS "count!"
FROM pg_inherits inheritance
JOIN pg_class child ON child.oid = inheritance.inhrelid
JOIN pg_namespace ns ON ns.oid = child.relnamespace
WHERE child.relname = $1
  AND ns.nspname = current_schema();
