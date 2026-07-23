# prompt-ferry

`prompt-ferry` is an OpenAI-compatible relay for Codex and other API clients.
It forwards requests through an outbound worker WebSocket to one or more
OpenAI-compatible or Anthropic-compatible upstream endpoints.

```text
Client -> relay /v1/* <- worker WebSocket -> upstream API
```

The project supports a single-host `serve` process and a managed deployment
with separate relay, worker, PostgreSQL, and admin-console services.

## Features

- OpenAI Chat Completions and Responses compatibility endpoints.
- Anthropic Messages upstream support through the Responses endpoint.
- Managed users, client API keys, endpoint configuration, and model routing.
- Multiple relays connected to one managed worker.
- MCP aggregation for HTTP and stdio servers.
- Request usage logging, replayable Responses conversations, and retention.
- Configurable redaction for forwarded content, logs, and usage details.
- Optional LLM review and manual approval for AI requests.
- TLS, mutual TLS, and application-layer relay-worker encryption.

## Quick Start

### Docker Compose

The Compose example runs a managed local deployment with PostgreSQL, one relay,
and one worker. It uses the public image published to GHCR.

```bash
cp .env.example .env
```

Edit `.env` and replace every placeholder. Generate secrets with commands such
as:

```bash
openssl rand -hex 24
openssl rand -hex 32
openssl rand -base64 32
```

Set `PROMPT_FERRY_IMAGE` to the release image for your GitHub repository, for
example `ghcr.io/OWNER/prompt-ferry:latest`.
After the first tag release, set the repository's GHCR package visibility to
Public if GitHub created it as a private package.

Start the stack:

```bash
docker compose pull
docker compose up -d
docker compose logs -f relay worker
```

Open the admin console at <http://127.0.0.1:8787>. Sign in with the bootstrap
admin credentials from `.env`.

After signing in:

1. Add an upstream endpoint and its API key.
2. Create a model route for the models clients will use.
3. Create a user and a client API key.
4. Point the client at the relay:

```text
OPENAI_BASE_URL=http://127.0.0.1:8787/v1
OPENAI_API_KEY=<generated-client-key>
```

Useful commands:

```bash
docker compose ps
docker compose logs -f worker
docker compose restart worker
docker compose down
```

The worker admin fallback is available at <http://127.0.0.1:8789>. Keep it
bound to localhost unless it is protected by an authenticated reverse proxy.

### Prebuilt image

The release workflow publishes both `v<version>` and `latest` tags. Pull a
specific release directly when needed:

```bash
docker pull ghcr.io/OWNER/prompt-ferry:v0.5.0
```

### Single-host binary

Download the matching archive from the [GitHub Releases](https://github.com/OWNER/prompt-ferry/releases)
page, extract `prompt-ferry`, and start a relay plus worker in one process:

```bash
./prompt-ferry serve
```

For non-managed mode, configure the relay client token and the upstream worker
settings before starting:

```dotenv
PROMPT_FERRY_RELAY__BIND=127.0.0.1:8787
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<client-token>
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://api.example.com
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<upstream-api-key>
```

The published binary targets are:

- Linux x86_64
- Linux arm64
- macOS arm64
- Windows x86_64
- Windows arm64

## Configuration

Environment variables use the `PROMPT_FERRY_` prefix. Nested configuration
fields use double underscores.

### Relay

```dotenv
PROMPT_FERRY_RELAY__BIND=0.0.0.0:8787
PROMPT_FERRY_RELAY__WORKER_BIND=0.0.0.0:8788
PROMPT_FERRY_RELAY__CLIENT_TOKEN=<client-token>
PROMPT_FERRY_RELAY__WORKER_TOKEN=<worker-token>
PROMPT_FERRY_RELAY__REQUEST_TIMEOUT_SECONDS=300
PROMPT_FERRY_RELAY__TLS_MODE=off
PROMPT_FERRY_RELAY__BRIDGE_ENCRYPTION_MODE=off
```

### Worker

```dotenv
PROMPT_FERRY_WORKER__RELAY_URLS=["ws://relay:8788/ws/worker"]
PROMPT_FERRY_WORKER__WORKER_TOKEN=<worker-token>
PROMPT_FERRY_WORKER__UPSTREAM_BASE_URL=https://api.example.com
PROMPT_FERRY_WORKER__UPSTREAM_API_KEY=<upstream-api-key>
PROMPT_FERRY_WORKER__CONNECT_TIMEOUT_SECONDS=30
PROMPT_FERRY_WORKER__TLS_MODE=auto
PROMPT_FERRY_WORKER__BRIDGE_ENCRYPTION_MODE=off
```

Managed mode additionally requires:

```dotenv
PROMPT_FERRY_WORKER__DATABASE_URL=postgres://user:password@host:5432/database
PROMPT_FERRY_WORKER__ADMIN_BIND=127.0.0.1:8789
PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_LOGIN=admin
PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD=<admin-password>
PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY=<base64-32-byte-key>
```

The managed worker applies database migrations at startup. Back up PostgreSQL
before upgrades, and stop old workers before applying migrations documented as
destructive storage changes.

Valkey is optional. When it is unavailable, the worker uses local memory for
the supported cache and scheduler fallbacks while PostgreSQL remains the
durable control plane.

## Reverse Proxy and TLS

Expose only the relay HTTP port through a reverse proxy. Keep the relay-worker
WebSocket on the private container network, or route it only between trusted
hosts. For streaming AI responses, use HTTP/1.1, disable response buffering,
allow request bodies of at least 256 MiB, and set read and send timeouts longer
than the longest upstream request.

For separate relay and worker hosts, use `wss://` and configure either server
TLS or mutual TLS. A server-TLS worker connection needs settings like:

```dotenv
PROMPT_FERRY_RELAY__WORKER_TLS_MODE=server
PROMPT_FERRY_RELAY__WORKER_TLS_CERT=/run/secrets/relay.crt
PROMPT_FERRY_RELAY__WORKER_TLS_KEY=/run/secrets/relay.key
PROMPT_FERRY_WORKER__TLS_MODE=server
PROMPT_FERRY_WORKER__RELAY_URLS=["wss://relay.example.com:8788/ws/worker"]
PROMPT_FERRY_WORKER__RELAY_CA=/run/secrets/ca.crt
```

For mutual TLS, use `mtls` on both sides and additionally configure the
worker client certificate and key plus the relay worker client CA. Keep
certificate and key files outside the repository.

## API Compatibility

Supported public routes include:

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `GET /v1/models`
- `GET /healthz`
- `GET /ws/worker`
- `POST /mcp`
- `POST /mcp/{server}`

Responses requests support text, reasoning parts, function tools, tool choice,
parallel tool calls, JSON formats, `prompt_cache_key`, and bridge-side
`conversation` continuation.

The current compatibility layer does not support `input_file`, file-id image
inputs, audio input/output, non-function tools, `reasoning.summary`,
`truncation`, or `background`. The standalone `/v1/conversations*` resource
APIs are not implemented.

Anthropic upstreams use `POST /v1/messages` and are available through
`POST /v1/responses`. Chat Completions requests are not translated to
Anthropic upstreams.

## Managed Features

Managed mode provides:

- Admin and user-scoped endpoints, model routes, MCP servers, and API keys.
- User-scoped model visibility through `GET /v1/models`.
- Endpoint and API-key selection for conversation routing.
- Usage detail, request replay, retention cleanup, and redaction settings.
- Optional review policies: allow, manual approval, fail-open, and fail-closed.
- Relay-side public IP allowlisting.

Request bodies up to 256 MiB are accepted on the AI endpoints. Configure the
same or a larger limit on any reverse proxy in front of the relay.

## Production Security

- Replace every example token and password before exposing the relay.
- Do not expose the relay worker WebSocket or worker admin port publicly.
- Put the public relay behind HTTPS with an authenticated reverse proxy.
- Use `wss://`, server TLS, or mutual TLS for relay-worker traffic across hosts.
- Keep `PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY` in a secret manager and
  back it up with the database encryption-key policy.
- Treat raw request and response logging as sensitive data. Configure retention
  and restrict PostgreSQL access accordingly.
- Disable unnecessary request-content logging in production.
- Use a PostgreSQL backup and restore procedure before destructive migrations.

See [SECURITY.md](SECURITY.md) for vulnerability reporting. See
[CONTRIBUTING.md](CONTRIBUTING.md) for local development and test instructions.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE).
