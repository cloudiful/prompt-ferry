update workers
set last_heartbeat_at = now(),
    config_applied_revision = $3,
    config_restart_required = $4,
    coordination_mode = $5
where worker_id = $1
  and incarnation = $2
  and stopped_at is null
