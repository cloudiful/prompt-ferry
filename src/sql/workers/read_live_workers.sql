-- Read the live registry members that admission must be compatible with.
--
-- `incompatible_count` counts every live worker that is not in shared mode or
-- that applied a different shared-Valkey revision than the candidate. A
-- bootstrap candidate uses the total live count and must stay alone.
select
    count(*)::bigint as live_count,
    count(*) filter (
        where coordination_mode <> 'shared'
           or config_applied_revision is distinct from $3
    )::bigint as incompatible_count
from workers
where stopped_at is null
  and last_heartbeat_at >= now() - ($1::bigint * interval '1 second')
  and incarnation <> $2
