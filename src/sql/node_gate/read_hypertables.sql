SELECT
    h.hypertable_name,
    d.column_name,
    d.time_interval = INTERVAL '1 day' AS is_daily
FROM timescaledb_information.hypertables h
JOIN timescaledb_information.dimensions d
  ON d.hypertable_schema = h.hypertable_schema
 AND d.hypertable_name = h.hypertable_name
WHERE h.hypertable_schema = 'public'
  AND h.hypertable_name IN (
      'request_records',
      'request_record_content',
      'request_record_raw_payloads',
      'request_record_block_refs',
      'usage_prompt_blocks',
      'request_record_assistant_artifacts',
      'request_record_tool_calls',
      'request_record_replay_snapshots'
  )
ORDER BY h.hypertable_name
