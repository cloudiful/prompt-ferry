-- Acquire the same transaction-scoped advisory key the chunk maintenance path
-- uses, before any decompression or row DML. Transaction scope means a failed
-- clear releases the key on rollback and a successful clear holds it until
-- commit.
SELECT pg_try_advisory_xact_lock($1) AS "acquired!";
