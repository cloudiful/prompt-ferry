-- no-transaction
-- Issue #794 Phase P4: cover the `endpoint_api_keys` lookup that deleting a key
-- drives on `conversation_endpoint_overrides`. The foreign key is
-- `ON DELETE SET NULL`, and the existing index on this table leads with
-- `endpoint_id`, so it cannot serve a lookup by the key column.
--
-- Built concurrently so production writes stay unblocked; sqlx runs this file
-- outside a transaction because `-- no-transaction` is the first line, and the
-- file holds exactly one statement because a multi-statement batch would be an
-- implicit transaction block that `CONCURRENTLY` refuses.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_conversation_endpoint_overrides_endpoint_key_id
    ON conversation_endpoint_overrides (endpoint_key_id);
