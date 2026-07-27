DO $$
DECLARE
    invalid_rules TEXT;
BEGIN
    SELECT string_agg(
        format('price_rule_id=%s NULL columns=%s', price_rule_id, null_columns),
        '; ' ORDER BY price_rule_id
    )
    INTO invalid_rules
    FROM (
        SELECT
            price_rule_id,
            concat_ws(
                ', ',
                CASE WHEN price_side IS NULL THEN 'price_side' END,
                CASE WHEN input_rate IS NULL THEN 'input_rate' END,
                CASE WHEN cache_read_rate IS NULL THEN 'cache_read_rate' END,
                CASE WHEN cache_write_rate IS NULL THEN 'cache_write_rate' END,
                CASE WHEN output_rate IS NULL THEN 'output_rate' END,
                CASE WHEN currency IS NULL THEN 'currency' END,
                CASE WHEN effective_from IS NULL THEN 'effective_from' END,
                CASE WHEN enabled IS NULL THEN 'enabled' END,
                CASE WHEN created_at IS NULL THEN 'created_at' END,
                CASE WHEN updated_at IS NULL THEN 'updated_at' END
            ) AS null_columns
        FROM billing_price_rules
        WHERE price_side IS NULL
           OR input_rate IS NULL
           OR cache_read_rate IS NULL
           OR cache_write_rate IS NULL
           OR output_rate IS NULL
           OR currency IS NULL
           OR effective_from IS NULL
           OR enabled IS NULL
           OR created_at IS NULL
           OR updated_at IS NULL
    ) AS invalid;

    IF invalid_rules IS NOT NULL THEN
        RAISE EXCEPTION
            'cannot enforce billing_price_rules NOT NULL constraints; repair these rows first: %',
            invalid_rules;
    END IF;
END
$$;

ALTER TABLE billing_price_rules
    ALTER COLUMN price_side SET NOT NULL,
    ALTER COLUMN input_rate SET NOT NULL,
    ALTER COLUMN cache_read_rate SET NOT NULL,
    ALTER COLUMN cache_write_rate SET NOT NULL,
    ALTER COLUMN output_rate SET NOT NULL,
    ALTER COLUMN currency SET NOT NULL,
    ALTER COLUMN effective_from SET NOT NULL,
    ALTER COLUMN enabled SET NOT NULL,
    ALTER COLUMN created_at SET NOT NULL,
    ALTER COLUMN updated_at SET NOT NULL;
