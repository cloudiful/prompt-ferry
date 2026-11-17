-- Replace the restorable settings rows.
--
-- The internal coordination rows are kept: the snapshot version counter
-- belongs to the running deployment and an import must never reset it.
delete from worker_settings
where setting_key <> all($1)
