-- Workers sharing one database discover each other through this registry.
-- Relay connection targets are local worker configuration, so managed relay
-- rows and their admin surface are removed in the same migration.

create table if not exists workers (
  worker_id text primary key,
  role text not null,
  version text not null,
  incarnation uuid not null,
  started_at timestamptz not null default now(),
  last_heartbeat_at timestamptz not null default now(),
  stopped_at timestamptz,
  config_applied_revision bigint,
  config_restart_required boolean not null default false,
  constraint workers_worker_id_check check (length(worker_id) between 1 and 200)
);

create index if not exists workers_heartbeat_idx
  on workers (last_heartbeat_at desc);

drop table if exists managed_relays;
