-- Issue #277 Phase P8: direct children of a partition parent, resolved
-- through the connection search_path so tests operate inside their schema.
SELECT child.relname AS "name!"
FROM pg_inherits inheritance
JOIN pg_class parent ON parent.oid = inheritance.inhparent
JOIN pg_class child ON child.oid = inheritance.inhrelid
WHERE parent.oid = ($1::text)::regclass
ORDER BY child.relname;
