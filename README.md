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
- Native Responses passthrough, including DeepSeek v4 Flash; use `Responses` or
  `Auto` endpoints with `force_passthrough` model routes.
- Bounded relay response buffering with configurable queue and byte limits.

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

The worker image includes `uv`/`uvx` for stdio MCP servers. For example, enter
`["uvx", "minimax-coding-plan-mcp", "-y"]` as the command. Environment variables
not configured in the MCP form are inherited from the worker. Sensitive variables
should be placed in `.env`, such as `MINIMAX_API_KEY`; Compose passes it to the
worker. Values can also be entered directly in the MCP form.

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
