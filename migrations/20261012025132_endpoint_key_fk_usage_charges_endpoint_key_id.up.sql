-- no-transaction
-- Issue #794 Phase P4: cover the `endpoint_api_keys` lookup that deleting a key
-- drives on `usage_charges`. The foreign key is `ON DELETE SET NULL`, and with
-- no index on its own column PostgreSQL has to scan the whole table to find the
-- rows to null out, once per key that is removed.
--
-- Built concurrently so production writes stay unblocked; sqlx runs this file
-- outside a transaction because `-- no-transaction` is the first line. That
-- directive is what makes the statement legal at all: a file sent as one batch
-- would be an implicit transaction block, which `CONCURRENTLY` refuses, so this
-- file holds exactly one statement.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_usage_charges_endpoint_key_id
    ON usage_charges (endpoint_key_id);
