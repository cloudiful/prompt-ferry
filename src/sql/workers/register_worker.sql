insert into workers (
  worker_id,
  role,
  version,
  incarnation,
  started_at,
  last_heartbeat_at,
  stopped_at,
  config_applied_revision,
  config_restart_required,
  coordination_mode
)
values ($1, $2, $3, $4, now(), now(), null, $5, $6, $7)
on conflict (worker_id) do update set
  role = excluded.role,
  version = excluded.version,
  incarnation = excluded.incarnation,
  started_at = now(),
  last_heartbeat_at = now(),
  stopped_at = null,
  config_applied_revision = excluded.config_applied_revision,
  config_restart_required = excluded.config_restart_required,
  coordination_mode = excluded.coordination_mode
where workers.incarnation = excluded.incarnation
  or workers.stopped_at is not null
  or workers.last_heartbeat_at < now() - ($8::bigint * interval '1 second')
