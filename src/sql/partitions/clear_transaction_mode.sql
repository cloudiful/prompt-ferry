-- Explicit isolation for the atomic request-family clear.
--
-- READ COMMITTED is the default, but the clear states it so the semantics do
-- not depend on a future default change: the scoped orphan NOT EXISTS check
-- must see refs committed by concurrent writers before the family delete, not
-- a transaction snapshot taken before the chunk decompression.
SET TRANSACTION ISOLATION LEVEL READ COMMITTED;
