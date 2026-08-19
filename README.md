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
- Managed users, client API keys, upstream endpoints, model routes, and multiple relays.
- MCP aggregation for HTTP and stdio servers, request usage and replay, retention, approval, and billing.
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
`ghcr.io/cloudiful/prompt-ferry:latest`, and replace every secret placeholder.
Then start the stack:

```bash
docker compose pull
docker compose up -d
```

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

### Single-host binary

Download a release binary from [GitHub Releases](https://github.com/cloudiful/prompt-ferry/releases)
and run the relay and worker together:

```bash
./prompt-ferry serve
```

For non-managed mode, configure the relay client token and upstream worker
settings before starting:

```dotenv
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<client-token>
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://api.example.com
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<upstream-api-key>
```
