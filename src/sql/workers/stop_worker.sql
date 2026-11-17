update workers
set stopped_at = now(),
    last_heartbeat_at = now()
where worker_id = $1
  and incarnation = $2
  and stopped_at is null
