create table if not exists managed_relays (
  relay_id uuid primary key,
  name text not null,
  relay_url text not null,
  enabled boolean not null default true,
  tls_mode text not null,
  bridge_encryption_mode text not null,
  relay_ca_ciphertext bytea,
  relay_ca_nonce bytea,
  relay_ca_key_version smallint,
  client_cert_ciphertext bytea,
  client_cert_nonce bytea,
  client_cert_key_version smallint,
  client_key_ciphertext bytea,
  client_key_nonce bytea,
  client_key_key_version smallint,
  bridge_encryption_key_ciphertext bytea,
  bridge_encryption_key_nonce bytea,
  bridge_encryption_key_key_version smallint,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint managed_relays_tls_mode_check
    check (tls_mode in ('off', 'server', 'mtls')),
  constraint managed_relays_bridge_encryption_mode_check
    check (bridge_encryption_mode in ('off', 'required'))
);

create unique index if not exists managed_relays_relay_url_unique
  on managed_relays (relay_url);
