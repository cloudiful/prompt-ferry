-- One database-wide configuration revision. A trigger on every configuration
-- table bumps it and notifies the listening nodes inside the same transaction,
-- so a rollback cancels both the bump and the notification. The trigger column
-- lists are the configuration columns only: write-time columns, primary keys,
-- and runtime state are excluded.

create table public.prompt_ferry_config_revision (
    id smallint primary key,
    revision bigint not null default 0,
    last_notify_txid text,
    updated_at timestamptz not null default now(),
    constraint ck_prompt_ferry_config_revision_id check (id = 1),
    constraint ck_prompt_ferry_config_revision_revision check (revision >= 0)
);

insert into public.prompt_ferry_config_revision (id) values (1);

-- Bump the revision and queue the notification once per modifying transaction.
-- A configuration import writes many tables inside one transaction; the stored
-- transaction id suppresses every later bump of that same transaction.
create function public.prompt_ferry_bump_config_revision()
returns trigger
language plpgsql
set search_path = pg_catalog, public
as $$
declare
    transaction_id text := pg_current_xact_id()::text;
    previous_transaction_id text;
    next_revision bigint;
begin
    select last_notify_txid
      into previous_transaction_id
      from public.prompt_ferry_config_revision
     where id = 1
       for update;

    if not found then
        raise exception 'prompt_ferry_config_revision is missing its singleton row';
    end if;

    if previous_transaction_id is not distinct from transaction_id then
        return null;
    end if;

    update public.prompt_ferry_config_revision
       set revision = revision + 1,
           last_notify_txid = transaction_id,
           updated_at = now()
     where id = 1
    returning revision into next_revision;

    perform pg_notify('prompt_ferry_config_revision', next_revision::text);
    return null;
end;
$$;

create trigger trg_cfg_rev_users_write
after insert or delete on public.users
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_users_update
after update of login_name, password_hash, display_name, is_admin, is_active on public.users
for each row
when (
    old.login_name is distinct from new.login_name
    or old.password_hash is distinct from new.password_hash
    or old.display_name is distinct from new.display_name
    or old.is_admin is distinct from new.is_admin
    or old.is_active is distinct from new.is_active
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_client_keys_write
after insert or delete on public.client_keys
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_client_keys_update
after update of user_id, key_prefix, key_hash, label, enabled, secret on public.client_keys
for each row
when (
    old.user_id is distinct from new.user_id
    or old.key_prefix is distinct from new.key_prefix
    or old.key_hash is distinct from new.key_hash
    or old.label is distinct from new.label
    or old.enabled is distinct from new.enabled
    or old.secret is distinct from new.secret
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_provider_endpoints_write
after insert or delete on public.provider_endpoints
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_provider_endpoints_update
after update of scope, owner_user_id, name, base_url, native_api, native_api_source, api_key, enabled, key_lb_enabled, provider, provider_region, mcp_enabled, service_tier, proxy_url, active_windows, admin_api_key on public.provider_endpoints
for each row
when (
    old.scope is distinct from new.scope
    or old.owner_user_id is distinct from new.owner_user_id
    or old.name is distinct from new.name
    or old.base_url is distinct from new.base_url
    or old.native_api is distinct from new.native_api
    or old.native_api_source is distinct from new.native_api_source
    or old.api_key is distinct from new.api_key
    or old.enabled is distinct from new.enabled
    or old.key_lb_enabled is distinct from new.key_lb_enabled
    or old.provider is distinct from new.provider
    or old.provider_region is distinct from new.provider_region
    or old.mcp_enabled is distinct from new.mcp_enabled
    or old.service_tier is distinct from new.service_tier
    or old.proxy_url is distinct from new.proxy_url
    or old.active_windows is distinct from new.active_windows
    or old.admin_api_key is distinct from new.admin_api_key
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_endpoint_api_keys_write
after insert or delete on public.endpoint_api_keys
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_endpoint_api_keys_update
after update of endpoint_id, key_label, api_key, position, enabled on public.endpoint_api_keys
for each row
when (
    old.endpoint_id is distinct from new.endpoint_id
    or old.key_label is distinct from new.key_label
    or old.api_key is distinct from new.api_key
    or old.position is distinct from new.position
    or old.enabled is distinct from new.enabled
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_endpoint_oauth_tokens_write
after insert or delete on public.endpoint_oauth_tokens
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_endpoint_oauth_tokens_update
after update of access_token, refresh_token, expires_at on public.endpoint_oauth_tokens
for each row
when (
    old.access_token is distinct from new.access_token
    or old.refresh_token is distinct from new.refresh_token
    or old.expires_at is distinct from new.expires_at
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_model_endpoint_rules_write
after insert or delete on public.model_endpoint_rules
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_model_endpoint_rules_update
after update of scope, owner_user_id, model_pattern, endpoint_id, priority, enabled, routing_strategy on public.model_endpoint_rules
for each row
when (
    old.scope is distinct from new.scope
    or old.owner_user_id is distinct from new.owner_user_id
    or old.model_pattern is distinct from new.model_pattern
    or old.endpoint_id is distinct from new.endpoint_id
    or old.priority is distinct from new.priority
    or old.enabled is distinct from new.enabled
    or old.routing_strategy is distinct from new.routing_strategy
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_model_route_targets_write
after insert or delete on public.model_route_targets
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_model_route_targets_update
after update of rule_id, endpoint_id, position, enabled, upstream_model, proxy_url_override, active_windows, dev_system_normalize, native_api, thinking_effort_override, compact_mode, thinking_downgrade_enabled, service_tier, embedding_dimensions_override, prefer_new_sessions on public.model_route_targets
for each row
when (
    old.rule_id is distinct from new.rule_id
    or old.endpoint_id is distinct from new.endpoint_id
    or old.position is distinct from new.position
    or old.enabled is distinct from new.enabled
    or old.upstream_model is distinct from new.upstream_model
    or old.proxy_url_override is distinct from new.proxy_url_override
    or old.active_windows is distinct from new.active_windows
    or old.dev_system_normalize is distinct from new.dev_system_normalize
    or old.native_api is distinct from new.native_api
    or old.thinking_effort_override is distinct from new.thinking_effort_override
    or old.compact_mode is distinct from new.compact_mode
    or old.thinking_downgrade_enabled is distinct from new.thinking_downgrade_enabled
    or old.service_tier is distinct from new.service_tier
    or old.embedding_dimensions_override is distinct from new.embedding_dimensions_override
    or old.prefer_new_sessions is distinct from new.prefer_new_sessions
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_mcp_servers_write
after insert or delete on public.mcp_servers
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_mcp_servers_update
after update of name, transport, url, command, args, env_json, enabled, timeout_ms, disabled_tools, disabled_resources, scope, owner_user_id, tool_filter_mode, allowed_tools, http_headers_json, aggregate_naming_mode, bearer_tokens_json, lifecycle_policy, lifecycle_manual_protocol_version, source_endpoint_id, auth_mode, basic_username, basic_password, provider_kind, proxy_url on public.mcp_servers
for each row
when (
    old.name is distinct from new.name
    or old.transport is distinct from new.transport
    or old.url is distinct from new.url
    or old.command is distinct from new.command
    or old.args is distinct from new.args
    or old.env_json is distinct from new.env_json
    or old.enabled is distinct from new.enabled
    or old.timeout_ms is distinct from new.timeout_ms
    or old.disabled_tools is distinct from new.disabled_tools
    or old.disabled_resources is distinct from new.disabled_resources
    or old.scope is distinct from new.scope
    or old.owner_user_id is distinct from new.owner_user_id
    or old.tool_filter_mode is distinct from new.tool_filter_mode
    or old.allowed_tools is distinct from new.allowed_tools
    or old.http_headers_json is distinct from new.http_headers_json
    or old.aggregate_naming_mode is distinct from new.aggregate_naming_mode
    or old.bearer_tokens_json is distinct from new.bearer_tokens_json
    or old.lifecycle_policy is distinct from new.lifecycle_policy
    or old.lifecycle_manual_protocol_version is distinct from new.lifecycle_manual_protocol_version
    or old.source_endpoint_id is distinct from new.source_endpoint_id
    or old.auth_mode is distinct from new.auth_mode
    or old.basic_username is distinct from new.basic_username
    or old.basic_password is distinct from new.basic_password
    or old.provider_kind is distinct from new.provider_kind
    or old.proxy_url is distinct from new.proxy_url
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_mcp_credentials_write
after insert or delete on public.mcp_credentials
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_mcp_credentials_update
after update of server_id, credential_label, secret, position, enabled, provider_kind, default_cost, strict_mode on public.mcp_credentials
for each row
when (
    old.server_id is distinct from new.server_id
    or old.credential_label is distinct from new.credential_label
    or old.secret is distinct from new.secret
    or old.position is distinct from new.position
    or old.enabled is distinct from new.enabled
    or old.provider_kind is distinct from new.provider_kind
    or old.default_cost is distinct from new.default_cost
    or old.strict_mode is distinct from new.strict_mode
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_user_redaction_configs_write
after insert or delete on public.user_redaction_configs
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_user_redaction_configs_update
after update of user_id, config on public.user_redaction_configs
for each row
when (
    old.user_id is distinct from new.user_id
    or old.config is distinct from new.config
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_user_endpoint_settings_write
after insert or delete on public.user_endpoint_settings
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_user_endpoint_settings_update
after update of user_id, endpoint_id on public.user_endpoint_settings
for each row
when (
    old.user_id is distinct from new.user_id
    or old.endpoint_id is distinct from new.endpoint_id
)
execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_worker_settings_write
after insert or delete on public.worker_settings
for each row execute function public.prompt_ferry_bump_config_revision();

create trigger trg_cfg_rev_worker_settings_update
after update of setting_key, setting_value on public.worker_settings
for each row
when (
    old.setting_key is distinct from new.setting_key
    or old.setting_value is distinct from new.setting_value
)
execute function public.prompt_ferry_bump_config_revision();
