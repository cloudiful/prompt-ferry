UPDATE usage_charges
SET input_tokens = GREATEST(input_tokens, 0),
    cache_read_tokens = GREATEST(cache_read_tokens, 0),
    cache_write_tokens = GREATEST(cache_write_tokens, 0),
    output_tokens = GREATEST(output_tokens, 0);

ALTER TABLE usage_charges
    ADD CONSTRAINT ck_usage_charges_token_counts_nonnegative
    CHECK (
        input_tokens >= 0
        AND cache_read_tokens >= 0
        AND cache_write_tokens >= 0
        AND output_tokens >= 0
    );
