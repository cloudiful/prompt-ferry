-- Read the persisted revision of a shared setting without touching its
-- secret envelope. The revision is a plain field of the stored JSON; a row
-- written before the revision existed counts as revision zero.
--
-- The row is only returned when the setting exists, so a missing setting and
-- a stored revision of zero stay distinguishable for the compare-and-set
-- write.
select coalesce((setting_value->>'revision')::bigint, 0) as revision
from worker_settings
where setting_key = $1
