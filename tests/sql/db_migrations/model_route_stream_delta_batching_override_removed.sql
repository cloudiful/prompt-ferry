SELECT
    NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'model_endpoint_rules'
          AND column_name = 'stream_output_coalescing_override_json'
    ) AS missing;
