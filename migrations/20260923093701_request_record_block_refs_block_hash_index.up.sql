-- no-transaction
-- Issue #277 Phase P5: cover the orphan prompt-block anti-join.
-- `request_record_block_refs` carries no index on `block_hash`, so orphan
-- cleanup did a full-table hash anti-join. Build the index concurrently so
-- production writes stay unblocked; sqlx runs this file outside a transaction
-- because the `-- no-transaction` directive is the first line.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_request_record_block_refs_block_hash
    ON request_record_block_refs (block_hash);
