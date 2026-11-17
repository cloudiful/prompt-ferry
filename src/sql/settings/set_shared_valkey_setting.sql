-- Persist a shared setting row with its revision.
--
-- The write happens inside the caller's transaction, so the advisory lock
-- taken there covers the read of the previous revision and this write.
insert into worker_settings (setting_key, setting_value, updated_at)
values ($1, $2::jsonb, now())
on conflict (setting_key) do update set
  setting_value = excluded.setting_value,
  updated_at = now()
