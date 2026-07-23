SELECT
  data_type,
  is_nullable = 'NO' AS is_not_null,
  column_default IS NOT NULL AS has_default
FROM information_schema.columns
WHERE table_schema = current_schema()
  AND table_name = 'mcp_servers'
  AND column_name = 'bearer_tokens_json';
