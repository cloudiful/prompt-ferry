-- Issue #653 operator recovery runbook (manual, one-off — not executed by any
-- migration runner).
--
-- Symptom this repairs
--   `prompt-ferry` fails to start with SQLx re-applying
--   `20260923093701_request_record_block_refs_block_hash_index`:
--       cannot create index on partitioned table "request_record_block_refs" concurrently
--   The database already ran `20260923135234_partition_request_family` (so the
--   table is a partitioned parent and the index already exists), but the earlier
--   index migration was never recorded in `_sqlx_migrations`.
--
-- When to run
--   Only against the state above. Every precondition below is verified and the
--   script runs in one transaction, so an unmet precondition or a ledger that
--   does not end in the expected shape raises and rolls back. The migration
--   ledger is never silently rewritten.
--
-- How to run
--   psql -v ON_ERROR_STOP=1 \
--     -f scripts/db/repair_request_record_block_refs_migration.sql
--
-- What it does NOT do
--   It does not build, validate, rebuild, or drop the index, and it writes no
--   ledger row other than the historical one below. The index stays owned by
--   `20260923135234_partition_request_family`.
--
-- Historical migration registered by this runbook
--   version      : 20260923093701
--   file         : migrations/20260923093701_request_record_block_refs_block_hash_index.up.sql
--   description  : request record block refs block hash index
--   checksum     : SHA-384 of the migration file as SQLx computes it; confirm with
--                  sha384sum migrations/20260923093701_request_record_block_refs_block_hash_index.up.sql
--   expected     : a3cdff5de82fb141e92d31aec45408750eac84444168bf07068d2c4f84ef4ccd9fb687de7d3100b61cdb22bacff49fed

BEGIN;

DO $repair$
DECLARE
    target_version      CONSTANT BIGINT := 20260923093701;
    target_description  CONSTANT TEXT   := 'request record block refs block hash index';
    target_checksum     CONSTANT TEXT   := 'a3cdff5de82fb141e92d31aec45408750eac84444168bf07068d2c4f84ef4ccd9fb687de7d3100b61cdb22bacff49fed';
    partition_version   CONSTANT BIGINT := 20260923135234;
    target_table_name   CONSTANT TEXT   := 'request_record_block_refs';
    target_index_name   CONSTANT TEXT   := 'idx_request_record_block_refs_block_hash';

    ledger_oid          OID := to_regclass('_sqlx_migrations');
    table_oid           OID := to_regclass(target_table_name);
    index_oid           OID := to_regclass(target_index_name);
    ledger_schema       NAME;
    table_schema        NAME;
    table_relkind       "char";
    index_is_valid      BOOLEAN;
    index_is_ready      BOOLEAN;
    index_table_oid     OID;
BEGIN
    IF ledger_oid IS NULL THEN
        RAISE EXCEPTION
            'refusing repair: _sqlx_migrations is not visible on the current search_path';
    END IF;

    IF table_oid IS NULL THEN
        RAISE EXCEPTION
            'refusing repair: relation % is not visible on the current search_path',
            target_table_name;
    END IF;

    IF index_oid IS NULL THEN
        RAISE EXCEPTION
            'refusing repair: index % is not visible on the current search_path',
            target_index_name;
    END IF;

    SELECT n.nspname INTO ledger_schema
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.oid = ledger_oid;

    SELECT n.nspname, c.relkind INTO table_schema, table_relkind
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE c.oid = table_oid;

    IF table_schema <> ledger_schema THEN
        RAISE EXCEPTION
            'refusing repair: % is in schema % but the migration ledger is in schema %',
            target_table_name, table_schema, ledger_schema;
    END IF;

    -- The table must already be the partitioned parent created by the partition
    -- migration; a plain table means the historical index migration can still
    -- run normally and must not be pre-registered.
    IF table_relkind <> 'p'::"char" THEN
        RAISE EXCEPTION
            'refusing repair: % has relkind %, expected a partitioned parent (p); run the normal migration path instead',
            target_table_name, table_relkind;
    END IF;

    -- The index must already exist on that exact parent and be fully valid, so
    -- the historical CONCURRENTLY build is genuinely unnecessary. An invalid or
    -- in-progress index (including a leftover from a failed concurrent build)
    -- must be resolved before the migration is registered.
    SELECT i.indisvalid, i.indisready, i.indrelid
    INTO index_is_valid, index_is_ready, index_table_oid
    FROM pg_index i
    WHERE i.indexrelid = index_oid;

    IF NOT FOUND THEN
        RAISE EXCEPTION
            'refusing repair: % is not an index', target_index_name;
    END IF;

    IF index_table_oid <> table_oid THEN
        RAISE EXCEPTION
            'refusing repair: index % is not on %',
            target_index_name, target_table_name;
    END IF;

    IF NOT index_is_valid OR NOT index_is_ready THEN
        RAISE EXCEPTION
            'refusing repair: index % exists on % but is not valid and ready; resolve the index before registering',
            target_index_name, target_table_name;
    END IF;

    -- The premise is that the partition migration ran ahead and was recorded;
    -- without its ledger row, registering only the earlier migration would
    -- leave SQLx to fail on the partition migration instead.
    IF NOT EXISTS (
        SELECT 1 FROM _sqlx_migrations
        WHERE version = partition_version AND success
    ) THEN
        RAISE EXCEPTION
            'refusing repair: % is partitioned but the partition migration % is not registered; recover that state first',
            target_table_name, partition_version;
    END IF;

    -- A failed attempt must be resolved explicitly: SQLx refuses to run while
    -- any row has success = false, and ON CONFLICT DO NOTHING cannot clear it.
    IF EXISTS (
        SELECT 1 FROM _sqlx_migrations
        WHERE version = target_version AND NOT success
    ) THEN
        RAISE EXCEPTION
            'refusing repair: migration % has a failed ledger row; resolve it explicitly instead of rewriting history',
            target_version;
    END IF;

    INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
    VALUES (target_version, target_description, TRUE, decode(target_checksum, 'hex'), -1)
    ON CONFLICT (version) DO NOTHING;

    IF FOUND THEN
        RAISE NOTICE 'registered historical migration % with the original checksum', target_version;
    ELSE
        RAISE NOTICE 'historical migration % was already registered; no change', target_version;
    END IF;
END
$repair$;

-- Fail closed: the ledger must now hold exactly one successful row for the
-- historical migration carrying the original checksum, so a repeat run is a
-- safe no-op and a mismatched existing row aborts the transaction.
DO $verify$
DECLARE
    target_version  CONSTANT BIGINT := 20260923093701;
    target_checksum CONSTANT TEXT   := 'a3cdff5de82fb141e92d31aec45408750eac84444168bf07068d2c4f84ef4ccd9fb687de7d3100b61cdb22bacff49fed';
    registered      INTEGER;
    checksum_ok     BOOLEAN;
BEGIN
    SELECT count(*), bool_and(checksum = decode(target_checksum, 'hex'))
    INTO registered, checksum_ok
    FROM _sqlx_migrations
    WHERE version = target_version AND success;

    IF registered <> 1 OR checksum_ok IS DISTINCT FROM TRUE THEN
        RAISE EXCEPTION
            'repair verification failed: expected exactly one successful row for % carrying the original checksum',
            target_version;
    END IF;
END
$verify$;

COMMIT;
