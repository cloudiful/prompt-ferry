select
  exists (
    select 1
    from information_schema.tables
    where table_schema = current_schema()
      and table_name = 'managed_relays'
  ) as table_exists,
  exists (
    select 1
    from information_schema.columns
    where table_schema = current_schema()
      and table_name = 'managed_relays'
      and column_name = 'relay_id'
      and data_type = 'uuid'
  ) as relay_id_is_uuid,
  exists (
    select 1
    from pg_constraint c
    join pg_class t on t.oid = c.conrelid
    join pg_namespace n on n.oid = t.relnamespace
    where n.nspname = current_schema()
      and t.relname = 'managed_relays'
      and c.conname = 'managed_relays_tls_mode_check'
  ) as tls_mode_has_check,
  exists (
    select 1
    from pg_constraint c
    join pg_class t on t.oid = c.conrelid
    join pg_namespace n on n.oid = t.relnamespace
    where n.nspname = current_schema()
      and t.relname = 'managed_relays'
      and c.conname = 'managed_relays_bridge_encryption_mode_check'
  ) as bridge_mode_has_check,
  exists (
    select 1
    from pg_indexes
    where schemaname = current_schema()
      and tablename = 'managed_relays'
      and indexname = 'managed_relays_relay_url_unique'
  ) as relay_url_unique;
