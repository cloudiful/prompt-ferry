-- Issue #653 Phase P2: transaction-safe, idempotent forward path for the
-- `request_record_block_refs.block_hash` index.
--
-- `20260923093701_request_record_block_refs_block_hash_index` builds the index
-- with `CREATE INDEX CONCURRENTLY`, which PostgreSQL rejects on a partitioned
-- parent. `20260923135234_partition_request_family` turns
-- `request_record_block_refs` into a daily RANGE(created_at) parent and creates
-- the same index non-concurrently, so the historical migration cannot be
-- re-applied once the partition migration is present. This migration is the
-- plain, in-transaction form: it is a no-op wherever the index already exists
-- and it never rebuilds an index owned by the partition migration.
CREATE INDEX IF NOT EXISTS idx_request_record_block_refs_block_hash
    ON request_record_block_refs (block_hash);
