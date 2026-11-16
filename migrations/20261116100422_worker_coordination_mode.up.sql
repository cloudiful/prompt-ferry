-- Record whether a registry row belongs to a shared-Valkey coordinated worker
-- or to the single-worker bootstrap mode. Admission compares the mode and the
-- applied shared revision across live members, so a stale local-mode process
-- and a coordinated process can never run side by side.
alter table workers add column coordination_mode text;

update workers
set coordination_mode = case
    when config_applied_revision is null then 'bootstrap'
    else 'shared'
end;

alter table workers alter column coordination_mode set not null;
alter table workers alter column coordination_mode set default 'bootstrap';
alter table workers add constraint workers_coordination_mode_check
    check (coordination_mode in ('bootstrap', 'shared'));
