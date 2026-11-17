-- Count the live worker rows that belong to another incarnation.
--
-- "Live" is derived from database time: a row without a recent heartbeat is
-- treated as expired and therefore as not running.
select count(*)::bigint as live_count
from workers
where stopped_at is null
  and last_heartbeat_at >= now() - ($1::bigint * interval '1 second')
  and incarnation <> $2
