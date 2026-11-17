-- Preserve the last allocated fence before the settings-based allocation
-- takes over again, so a downgrade never reuses a published version.
insert into worker_settings (setting_key, setting_value, updated_at)
select
    'relay_snapshot_version',
    jsonb_build_object('version', last_value),
    now()
from relay_snapshot_version_seq
on conflict (setting_key) do update set
    setting_value = excluded.setting_value,
    updated_at = now();

drop sequence relay_snapshot_version_seq;
