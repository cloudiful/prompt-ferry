SELECT EXISTS (
    SELECT 1
    FROM pg_indexes
    WHERE schemaname = current_schema()
      AND tablename = 'mcp_servers'
      AND indexname = 'idx_mcp_servers_scope_user_enabled'
) AS "exists!"
