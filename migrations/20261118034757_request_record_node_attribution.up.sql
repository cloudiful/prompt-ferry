-- The node ownership of a request record: the group of the star, the node the
-- request physically entered, and the node that processed it. The column is
-- nullable, so old rows and the standalone path keep no attribution. Adding it
-- to the partitioned parent recurses into the existing daily partitions, and
-- partitions created later inherit it.

alter table public.request_records
    add column if not exists node_attribution jsonb;
