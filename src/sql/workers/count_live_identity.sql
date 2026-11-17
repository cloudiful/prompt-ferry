-- Count the live rows that already own a worker identity.
--
-- A live row of another incarnation is a collision: the identity is taken and
-- must not be replaced by a second live process.
select count(*)::bigint as live_count
from workers
where worker_id = $1
  and stopped_at is null
  and last_heartbeat_at >= now() - ($2::bigint * interval '1 second')
  and incarnation <> $3
