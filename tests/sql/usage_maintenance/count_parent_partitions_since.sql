SELECT COUNT(*)::BIGINT AS "count!"
FROM pg_inherits inheritance
JOIN pg_class parent ON parent.oid = inheritance.inhparent
JOIN pg_class child ON child.oid = inheritance.inhrelid
JOIN pg_namespace ns ON ns.oid = parent.relnamespace
WHERE parent.relname = $1
  AND ns.nspname = current_schema()
  AND child.relname >= $2;
