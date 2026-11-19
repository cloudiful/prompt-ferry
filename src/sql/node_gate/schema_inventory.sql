SELECT
    to_regclass('public._sqlx_migrations') IS NOT NULL AS has_migrations_table,
    to_regclass('public.prompt_ferry_node_gate') IS NOT NULL AS has_gate_table,
    COALESCE((
        SELECT array_agg(c.relname ORDER BY c.relname)
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public'
          AND c.relkind IN ('r', 'p')
          AND (
              c.relname IN (
                  'request_records',
                  'request_record_content',
                  'request_record_raw_payloads',
                  'request_record_block_refs',
                  'usage_prompt_blocks',
                  'request_record_assistant_artifacts',
                  'request_record_tool_calls',
                  'request_record_replay_snapshots'
              )
              OR c.relname ~ '^(request_records|request_record_content|request_record_raw_payloads|request_record_block_refs|usage_prompt_blocks|request_record_assistant_artifacts|request_record_tool_calls|request_record_replay_snapshots)_[0-9]'
          )
    ), ARRAY[]::text[]) AS request_relations
