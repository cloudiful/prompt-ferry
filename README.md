# prompt-ferry

[![Release](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml/badge.svg)](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/cloudiful/prompt-ferry?display_name=tag)](https://github.com/cloudiful/prompt-ferry/releases)
[![License](https://img.shields.io/github/license/cloudiful/prompt-ferry)](LICENSE)
[![GHCR](https://img.shields.io/badge/container-GHCR-2496ED?logo=docker&logoColor=white)](https://github.com/cloudiful/prompt-ferry/pkgs/container/prompt-ferry)

[English](README.md) | [简体中文](README.zh-CN.md)

`prompt-ferry` is an OpenAI-compatible AI API relay for Codex and other API
clients. It can redact sensitive content before forwarding requests through a
relay-worker bridge to one or more OpenAI-compatible or Anthropic-compatible
upstreams.

```text
Client -> relay /v1/* <-> worker WebSocket -> upstream API
```

## Features

- OpenAI Chat Completions and Responses compatibility, plus native Anthropic Messages and Models endpoints.
- Anthropic SDK clients can use `POST /v1/messages` and `GET /v1/models` with `x-api-key`; native Messages
  requests are transparently forwarded only to endpoints configured as `AnthropicMessages`.
- Per-request reasoning controls: Chat `reasoning_effort` and Responses `reasoning.effort`, including DeepSeek `max`; Chat compatibility maps unsupported `developer` roles to `system`.
- Configurable redaction for forwarded content, logs, and usage details.
- Users, client API keys, upstream endpoints, model routes, and multiple relays.
- MCP aggregation for HTTP and stdio servers, with SQLite support for configuration, catalog, and runtime execution; MCP quota and usage ledgers require PostgreSQL.
- MCP credential quota: per-credential and shared quota-group budgets (requests or credits) with
  atomic reservation, usage-ratio balancing across API keys, cooldown on auth/throttle failures,
  and provider credit reconciliation (`creditsUsed`) for Firecrawl-style meters.
- TLS, mutual TLS, and application-layer encryption for relay-worker traffic.
- Native Responses passthrough, including DeepSeek v4 Flash; Responses requests
  require Responses-native targets and are forwarded without cross-protocol conversion.
- Bounded relay response buffering with configurable queue and byte limits.
- Relay readiness: `GET /ready` returns `200` only while a worker is connected and client routes are
  loaded; otherwise it fails fast with `503` and `Retry-After: 5` so SDKs retry during a rolling
  upgrade, while `/healthz` stays liveness-only.

## Deploy

### Docker Compose

The Compose example starts PostgreSQL, a relay, a worker, and the admin console
with a prebuilt image from GHCR.

```bash
cp .env.example .env
```

Edit `.env`, set `PROMPT_FERRY_IMAGE` to
`ghcr.io/cloudiful/prompt-ferry:latest`, and replace the remaining secret
placeholders. The worker token, encryption key, upstream API key, and
bootstrap admin password are optional; unset secrets are generated or
deferred to Admin setup as described under [Worker storage](#worker-storage).
Then start the stack:

```bash
docker compose pull
docker compose up -d
```

For MiniMax Coding Plan, prefer the MiniMax endpoint's `Expose MiniMax MCP tools`
switch (recommended). It creates a managed `builtin_minimax` server exposing
`web_search` and `understand_image` without spawning a `uvx` subprocess. The
managed server reuses the endpoint's token-plan API keys (multi-key rotation
when endpoint key load-balancing is on) and needs no Basic Auth; the endpoint
region selects `https://api.minimaxi.com` (CN) or `https://api.minimax.io`
(Global). Existing custom `minimax-coding-plan-mcp` stdio rows are left
untouched and keep their current behavior.

The worker image also includes `uv`/`uvx` for generic third-party stdio MCP
servers. For example, enter `["uvx", "example-mcp-server", "-y"]` as the
command. Environment variables not configured in the MCP form are inherited
from the worker. Sensitive variables should be placed in `.env`, such as
`MINIMAX_API_KEY`; Compose passes it to the worker. Values can also be entered
directly in the MCP form.

Compose persists the worker uv caches (`/root/.cache/uv` and
`/root/.local/share/uv`) in named volumes so recreating the container does not
re-download the Python runtime and dependencies. A plain container `restart`
keeps the filesystem; `down`/recreate without these volumes would lose the
caches.

Open the admin console at <http://127.0.0.1:8789>. After signing in, configure
an upstream endpoint, model route, user, and client API key. Point an
OpenAI-compatible client at the relay:

```dotenv
OPENAI_BASE_URL=http://127.0.0.1:8787/v1
OPENAI_API_KEY=<generated-client-key>
```

Anthropic SDK clients can use the same relay URL and generated client key:

```dotenv
ANTHROPIC_BASE_URL=http://127.0.0.1:8787
ANTHROPIC_API_KEY=<generated-client-key>
```

For OpenCode or other `@ai-sdk/anthropic` clients, use the relay `/v1` prefix
as the provider base URL so the SDK requests `/v1/messages`. Configure a
MiniMax Anthropic endpoint with `https://api.minimaxi.com/anthropic` or
`https://api.minimax.io/anthropic` as its upstream base URL and use the
`AnthropicMessages` protocol.

The first Anthropic-compatible release supports Messages and Models only. It does not translate
Anthropic Messages requests to OpenAI Chat or Responses endpoints, and does not expose Anthropic
Files, Batches, or Token Counting APIs.

For deployments that keep raw payloads outside PostgreSQL, set the
`PROMPT_FERRY_WORKER__RAW_OBJECT_STORE_*` variables in `.env` and select
`object_store` in the usage-retention settings. Configure the bucket with a
3-day lifecycle, server-side encryption, and private access. Object storage
credentials remain deployment configuration and are not exposed through the
admin API.

For slow downstream clients, tune the bounded relay response buffer with
`PROMPT_FERRY_RELAY__RESPONSE_STREAM_BUFFER` and
`PROMPT_FERRY_RELAY__RESPONSE_STREAM_MAX_BYTES`. The optional
`PROMPT_FERRY_RELAY__RESPONSE_STREAM_BACKPRESSURE_TIMEOUT_MS` controls how long
each response forwarding pump waits for a slow client before aborting; it
defaults to 5000 ms. The defaults are 256 queued chunks and 16 MiB per response;
all three values must be greater than zero.

Keep port `8789` private. See [.env.example](.env.example) for the available
Compose settings.

### Worker storage

The worker uses one Admin API and one configuration model with either backend.
A non-empty `PROMPT_FERRY_WORKER__DATABASE_URL` selects PostgreSQL for shared,
durable storage. An empty value selects SQLite for local durable configuration;
a configured but unavailable PostgreSQL database does not fall back to SQLite.
Both backends support users, encrypted secrets, endpoints, routes, relays,
settings, client keys, and MCP configuration/catalog/runtime. SQLite also
serves the Admin API, including authentication, but does not provide durable
request records, raw-payload retention, approvals, billing, replay history, or
MCP quota/usage ledgers.

SQLite is intended for a single worker. PostgreSQL remains the choice for
shared workers and the complete advanced persistence surface. Valkey is
optional and can provide shared coordination/cache acceleration; without it,
SQLite uses SQLite coordination and PostgreSQL uses its existing backend or
bounded local fallbacks according to the state semantics.

The worker and relay may run on separate machines with
`prompt-ferry relay` and `prompt-ferry worker`. The relay-worker bridge
protocol is unchanged: the worker needs network access to the relay's worker
bind, and clients need access to the relay's public bind. Configure relay URLs
with repeatable `--relay-url` options or the `relay_urls` configuration list
(for environment overrides, `PROMPT_FERRY_WORKER__RELAY_URLS` is a JSON array).

On first startup, an empty SQLite database is bootstrapped from the static
worker settings, including relay URLs, upstream base URL and API key, TLS, and
bridge-encryption settings; an upstream endpoint can also be created later
through the Admin setup flow. After bootstrap, the SQLite configuration is
authoritative. Reload polling applies supported direct SQLite changes without
restarting the worker. Secrets at rest are encrypted with a base64-encoded
32-byte worker configuration encryption key
(`PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY`; the legacy
`PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY` name is still accepted). When
unset, a random key is generated and persisted to
`<data-root>/prompt-ferry/worker-config.key` (`0600` on Unix). SQLite never
stores provider API keys in plaintext, and there is no plaintext fallback.

The default data root follows the SQLite database location below; generated
files live next to it under `prompt-ferry/`. When no active admin user exists
and no bootstrap password is configured, a strong random password is generated
and written once to `<data-root>/prompt-ferry/bootstrap-admin.txt`
(`0600` on Unix); only the file path and login are logged. Existing configured
bootstrap credentials take precedence, and existing users are never modified.

The relay `/ws/worker` endpoint requires `Authorization: Bearer <token>` when
a non-empty `WORKER_TOKEN` is configured. An empty token disables worker
authentication entirely — any client that can reach the worker bind can
connect as a worker — so use TLS and network isolation in that mode; the
relay logs a warning at startup.

The default SQLite database path is `$XDG_DATA_HOME/prompt-ferry/worker.sqlite3` or
`$HOME/.local/share/prompt-ferry/worker.sqlite3` on Linux,
`$HOME/Library/Application Support/prompt-ferry/worker.sqlite3` on macOS, and
`%LOCALAPPDATA%\\prompt-ferry\\worker.sqlite3` on Windows. Override it with
`PROMPT_FERRY_WORKER__STANDALONE_DATABASE_PATH` or
`--standalone-database-path`. Back up or restore the SQLite file while the
worker is stopped, and retain the worker configuration encryption key
(`worker-config.key`) with the backup.

SQLite request and usage summaries remain a bounded in-memory ring of 256
entries and are cleared on restart. Redaction rules persist, while
conversation-specific redaction sessions reset on restart. Direct SQLite edits
are picked up by reload polling only for supported schema/configuration changes
and must satisfy the normal secret-encryption constraints.

For a separate-host deployment, use placeholders like these and keep the
bridge port reachable from the worker host:

```dotenv
# Relay host
PROMPT_FERRY_RELAY__BIND=0.0.0.0:8787
PROMPT_FERRY_RELAY__WORKER_BIND=0.0.0.0:8788
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<client-token>
PROMPT_FERRY_RELAY__WORKER_TOKEN=<worker-token>
```

```bash
prompt-ferry relay
```

```dotenv
# Worker host
PROMPT_FERRY_WORKER__DATABASE_URL=
PROMPT_FERRY_WORKER__RELAY_URLS=["wss://relay.example.invalid:8788/ws/worker"]
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://upstream.example.invalid
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<upstream-api-key>
PROMPT_FERRY_WORKER__WORKER_TOKEN=<worker-token>
# Optional; auto-generated at <data-root>/prompt-ferry/worker-config.key when unset.
PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY=<base64-32-byte-key>
PROMPT_FERRY_WORKER__TLS_MODE=<configured-tls-mode>
PROMPT_FERRY_WORKER__BRIDGE_ENCRYPTION_MODE=<configured-bridge-mode>
```

```bash
prompt-ferry worker
```

### Outbound proxy

LLM upstreams use the endpoint `proxy_url` with per-route `proxy_url_override`
(`http/https/socks5/socks5h`); unset means direct. Model listing, token-plan
usage, the endpoint protocol check, and `me` available-models reuse the same
endpoint proxy via a pooled client; invalid proxy fails closed without
falling back to direct.

MCP HTTP servers resolve `mcp_servers.proxy_url` first, then the worker
process env (`HTTPS_PROXY`/`HTTP_PROXY`, lowercase accepted, `ALL_PROXY`
fallback) with `NO_PROXY` bypass, then direct. The built-in MiniMax MCP
inherits its source endpoint proxy (`endpoint → env → direct`); its row
proxy is ignored and image-URL fetches stay direct with pinned DNS. MCP
`stdio` inherits the worker env for the subprocess. An invalid proxy fails
the request without falling back to direct; credentials are never logged.

Stay direct by design: approval webhooks (user intranet facility), upstream
Realtime websockets (needs a hand-written CONNECT tunnel, deferred), and the
relay-worker bridge (internal control plane).

```dotenv
HTTPS_PROXY=http://proxy.example.invalid:8080
NO_PROXY=127.0.0.1,localhost
```

### Route target schedules

Each model-route target has optional active windows (`[{start,end,days?}]`
`HH:MM`, `days` 1=Mon..7=Sun omitted/empty means every day); empty means unrestricted. A target outside its windows never participates
in routing. Windows are evaluated in worker-local time: `end` before `start`
wraps overnight (e.g. `22:00–06:00`, weekday decided by start day), the start minute is
inclusive and the end minute is exclusive, and matching any window activates
the target. `enabled=false` targets never participate, even inside a window.

Run every worker in the same timezone when schedules matter; multi-worker
deployments with mixed local timezones evaluate the same windows differently.
Filtering is fail-closed: when no target is active the request fails instead
of falling back to an out-of-window target, and an invalid stored schedule
never silently becomes unrestricted.

```text
no route target is active for route 'summarizer' at 03:12 (worker-local time; windows: primary(enabled): 06:30–14:00, 18:00–20:00; night(disabled): unrestricted)
```

### Endpoint default schedules and target normalize

Each endpoint has optional default windows (`[{start,end,days?}]` `HH:MM`);
empty means unrestricted. A target with empty windows inherits its endpoint
default; a non-empty target overrides it; empty on both means unrestricted.
The same `HH:MM` validation applies to both levels.

Target normalize (`dev_system_normalize`, default off) controls Chat
passthrough: when off (default) `developer` is forwarded unchanged; when on
`developer` is rewritten to `system`. This is a behavior change from the
previous unconditional rewrite: strict upstreams that reject `developer`
need the switch turned on explicitly per target. The toggle lives in the
target-row gear popover alongside proxy and schedule, and is always sent as
`true`/`false`.

### Continuous-session cache alerts

PostgreSQL deployments can alert on conversations that keep rereading their
prompt context. (Standalone SQLite does not aggregate or alert; use
PostgreSQL.)

The monitor aggregates completed AI turns per `conversation_id` over the last
`window_minutes`: a conversation whose distinct-turn count reaches `min_turns`
and whose fold-aware cache read rate stays below `threshold` triggers one
DingTalk robot message. The rate is the usage overview rate
(`SUM(cache_read) / SUM(fold-aware full input)`, clamped to `0..1`), failed
and in-flight rows are ignored, and each conversation has its own
`cooldown_minutes` clock before it can alert again.

Configure it in the admin console (`GET`/`PUT /api/v1/settings/cache-alert`)
with `enabled`, `window_minutes` (5–1440, default 30), `min_turns` (2–100,
default 5), `threshold` (0–1, default 0.2), `cooldown_minutes` (5–1440,
default 60), plus the DingTalk robot `dingtalk_webhook_url` and optional
`dingtalk_secret` (signed robots). The secret is stored write-only and never
echoed back; leave it blank to keep the stored value. The monitor runs one
alerting pass per `min(window_minutes, 60)` minutes and only one worker
evaluates each pass.

Messages carry conversation metadata only — `conversation_id`, `model`,
`window`, `turns`, `cache_rate`, `threshold` — never request or user content.

### Responses compact

- `POST /v1/responses/compact` forwards to Responses-native upstreams
  byte-for-byte; feed the returned `output` back as the next
  `POST /v1/responses` `input` to continue the conversation.
- Targets without native compact support fall back to ferry-side
  summarization when `compact_mode=self_summarize` is set on the target
  (drops `encrypted_content`, trims old tool outputs, returns a plaintext
  handoff). The default `passthrough` rejects non-Responses targets with
  `400`; `off` disables compact for the target.

### Responses stateless fallback

When a Chat-compatible request carries a `tool_calls` turn whose `tool` output
was pruned or truncated, ferry inserts a `function_call_output` placeholder
(`[Missing tool output: pruned or truncated, call_id=<id>]`) right after the
call, so the upstream Responses API no longer rejects the request with
`No tool output found for function call`.

If the upstream still rejects a continuation (`No tool output found ...` or
`Referenced reasoning item ... was not found or has expired`), ferry answers
with `code=retryable_invalid_continuation` and a retry hint: drop
`previous_response_id`, truncate history before the orphan `call_*`, then
resend once. Both fallbacks emit structured warnings
(`event=chat_to_responses_missing_tool_output`,
`event=upstream_invalid_continuation`).

### Thinking downgrade

Thinking-mode upstreams reject a tool-bearing turn when the parent assistant
tool-call message has no reasoning to pass back (`reasoning_content` /
`reasoning_text` in the thinking mode must be passed back). Ferry keeps those
turns working:

- Pre-flight (only upstreams that require the echo, currently DeepSeek): when
  the stored parent artifact proves the parent turn produced no reasoning, the
  turn requests thinking, carries tools, and nothing restorable is present,
  ferry disables thinking for that single turn. Chat bodies get
  `thinking: {"type":"disabled"}` with `reasoning_effort` dropped (overriding a
  per-target thinking effort override); Responses bodies get
  `reasoning.effort: "none"`. Every other upstream (for example opencode go)
  keeps the requested thinking — including a per-target effort override — and
  its outbound body is never rewritten to `reasoning.effort: "none"`.
- Retry: an upstream `400` whose body contains `must be passed back` resends the
  same turn once with thinking off; if that resend is rejected too, the original
  upstream error is returned. This path is provider-independent and stays active
  for every upstream.

Neither path fabricates reasoning, stores anything new, or edits target config,
and a turn that can pass its reasoning back is forwarded byte-for-byte.
Observability events: `event=thinking_downgrade`, `event=thinking_echo_retry`,
`event=thinking_echo_retry_sent`, and `event=thinking_echo_retry_rejected`, each
carrying `conversation_id`, `provider`, `native_api`, `disposition`, and
`attempt`. Set `PROMPT_FERRY_DISABLE_THINKING_DOWNGRADE=1` to bypass both paths.

### Single-host binary

Download a release binary from [GitHub Releases](https://github.com/cloudiful/prompt-ferry/releases)
and run the relay and worker together:

```bash
./prompt-ferry serve
```

`serve` binds the internal worker bridge to loopback and starts with no
required secrets: an empty `PROMPT_FERRY_WORKER_TOKEN` keeps the bridge open
only on that loopback bind, the encryption key is generated on first start,
and the initial admin password (if needed) is written to
`<data-root>/prompt-ferry/bootstrap-admin.txt`. Configure a client token and
upstream endpoint through the Admin console at <http://127.0.0.1:8789>:

```dotenv
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<client-token>
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://api.example.com
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<upstream-api-key>
```
