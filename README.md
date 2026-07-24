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

- OpenAI Chat Completions and Responses compatibility, with Anthropic Messages upstream support through Responses.
- Configurable redaction for forwarded content, logs, and usage details.
- Managed users, client API keys, upstream endpoints, model routes, and multiple relays.
- MCP aggregation for HTTP and stdio servers, request usage and replay, retention, approval, and billing.
- TLS, mutual TLS, and application-layer encryption for relay-worker traffic.

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

## Documentation

- [Security policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)
- [Apache License 2.0](LICENSE)

## License

Licensed under the Apache License, Version 2.0.
