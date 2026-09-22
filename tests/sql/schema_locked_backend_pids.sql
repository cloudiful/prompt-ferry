SELECT pg_terminate_backend(locked.pid)
FROM (
    SELECT DISTINCT locks.pid
    FROM pg_locks locks
    JOIN pg_class relations ON relations.oid = locks.relation
    JOIN pg_namespace namespaces ON namespaces.oid = relations.relnamespace
    WHERE namespaces.nspname = $1
) locked
WHERE locked.pid <> pg_backend_pid()
