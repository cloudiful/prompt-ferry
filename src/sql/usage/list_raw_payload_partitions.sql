SELECT child.relname AS name
FROM pg_inherits inheritance
JOIN pg_class parent ON parent.oid = inheritance.inhparent
JOIN pg_class child ON child.oid = inheritance.inhrelid
WHERE parent.oid = 'request_record_raw_payloads'::regclass
  AND child.relname <> 'request_record_raw_payloads_default'
ORDER BY child.relname;
