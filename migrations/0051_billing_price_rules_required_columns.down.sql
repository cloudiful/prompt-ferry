ALTER TABLE billing_price_rules
    ALTER COLUMN price_side DROP NOT NULL,
    ALTER COLUMN input_rate DROP NOT NULL,
    ALTER COLUMN cache_read_rate DROP NOT NULL,
    ALTER COLUMN cache_write_rate DROP NOT NULL,
    ALTER COLUMN output_rate DROP NOT NULL,
    ALTER COLUMN currency DROP NOT NULL,
    ALTER COLUMN effective_from DROP NOT NULL,
    ALTER COLUMN enabled DROP NOT NULL,
    ALTER COLUMN created_at DROP NOT NULL,
    ALTER COLUMN updated_at DROP NOT NULL;
