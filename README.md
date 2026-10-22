# prompt-ferry

[![Release](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml/badge.svg)](https://github.com/cloudiful/prompt-ferry/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/cloudiful/prompt-ferry?display_name=tag)](https://github.com/cloudiful/prompt-ferry/releases)
[![License](https://img.shields.io/github/license/cloudiful/prompt-ferry)](LICENSE)
[![GHCR](https://img.shields.io/badge/container-GHCR-2496ED?logo=docker&logoColor=white)](https://github.com/cloudiful/prompt-ferry/pkgs/container/prompt-ferry)

[English](README.md) | [简体中文](README.zh-CN.md)

`prompt-ferry` is an OpenAI-compatible AI API relay for Codex and other clients.
It forwards requests to OpenAI-compatible or Anthropic-compatible upstreams over
a relay-worker bridge, redacting sensitive content on the way. A single binary
runs the relay and the worker together by default.

```text
Client -> relay /v1/* <-> worker WebSocket -> upstream API
```

## Capabilities

- OpenAI Chat Completions, Responses and Embeddings, plus native Anthropic Messages and Models.
- Upstream presets for MiniMax, CommandCode, OpencodeGo, OpenRouter, GLM, DeepSeek and OpenAI Platform; other OpenAI-compatible hosts use the Generic provider with an explicit base URL.
- Per-request reasoning controls, including Chat `reasoning_effort`, Responses `reasoning.effort`, and Anthropic `output_config.effort`.
- First-class Embeddings forwarding with per-target dimension overrides, fail-closed redaction and input-only metering ([docs](docs/embeddings.md)).
- Users, client API keys, upstream endpoints, model routes, key balancing, and multiple relays.
- Configurable redaction for forwarded content, logs and usage details.
- Admin console with effective default-text quota summaries, per-key detail, and 30-day OpenAI subscription history on PostgreSQL.
- MCP aggregation for HTTP and stdio servers, with per-credential and shared quota budgets.
- TLS, mutual TLS and application-layer encryption for relay-worker traffic.

Dependencies: PostgreSQL for business data, and optionally Valkey for
coordination and response affinity. Raw request and response payloads are
written to an S3-compatible bucket when one is configured.

## Quick start with Docker Compose (recommended)

```bash
cp .env.example .env
```

In `.env`, replace every `REPLACE...` placeholder: the database password (in
both `POSTGRES_PASSWORD` and `DATABASE_URL`), `PROMPT_FERRY_RELAY__CLIENT_TOKEN`,
and the two S3 credentials. The file is read as data and never expanded, so the
two database password entries have to be edited together.

Start the dependencies first, because the S3 bucket has to exist before the
application starts:

```bash
docker compose up -d postgres valkey rustfs
```

Open the RustFS console at <http://127.0.0.1:9001>, sign in with the S3 access
key and secret key from `.env`, and create a bucket named exactly as
`PROMPT_FERRY_WORKER__RAW_OBJECT_STORE_BUCKET` (`prompt-ferry-raw` by default).
RustFS serves S3 and does not create buckets for you. Then start the
application:

```bash
docker compose up -d prompt-ferry
```

- Admin console: <http://127.0.0.1:8789> — log in as `admin`.
- OpenAI-compatible base URL: `http://127.0.0.1:8787/v1`.

If you left `PROMPT_FERRY_WORKER__BOOTSTRAP_ADMIN_PASSWORD` empty and no admin
exists yet, a strong password was generated once and written to the data volume:

```bash
docker compose exec prompt-ferry cat /var/lib/prompt-ferry/prompt-ferry/bootstrap-admin.txt
```

## Running the binary directly

Download a release binary from [GitHub Releases](https://github.com/cloudiful/prompt-ferry/releases).
It needs the same PostgreSQL, Valkey and bucket, and it reads the same `.env`:

```bash
PROMPT_FERRY_DOTENV=.env prompt-ferry
```

With no subcommand the binary starts the integrated relay and worker. It keeps
its own state under `$XDG_DATA_HOME/prompt-ferry` and configuration under
`$XDG_CONFIG_HOME/prompt-ferry`. Stop the Compose application first, or the two
will fight over the published ports.

Fresh install and existing database differ. A fresh start with an empty
`PROMPT_FERRY_WORKER__WORKER_CONFIG_ENCRYPTION_KEY` generates a key once under
`<data-root>/prompt-ferry/worker-config.key`. To reuse a populated Compose
database, copy `/var/lib/prompt-ferry/prompt-ferry/worker-config.key` from the
Compose volume to the host `<data-root>/prompt-ferry/worker-config.key`
(default `~/.local/share/prompt-ferry/worker-config.key`) with owner-only
(`0600`) permissions before starting, and never replace/rotate it afterwards; or
set the same explicit base64 32-byte key in both environments before first
bootstrap. The existing database keeps its existing admin account — the
bootstrap password file is written only on initial creation when no admin exists.

## Before you expose it

- Back up PostgreSQL and `<data-root>/prompt-ferry/worker-config.key`; without that key, stored secrets cannot be decrypted.
- PostgreSQL 17 reaches 18 only through `pg_dump`/`pg_restore` or `pg_upgrade`. The example creates a fresh PostgreSQL 18 volume and migrates nothing.
- Everything publishes on loopback. Reaching it from another machine means a deliberate port binding, TLS and a reverse proxy.

## Documentation

[Embeddings](docs/embeddings.md) · [Quota feedback](docs/quota-feedback.md) · [Quota snapshots](docs/quota-snapshots.md) · [Endpoint key persistence](docs/endpoint-key-persistence.md) · [Frontend async actions](docs/frontend-async-actions.md) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md) · [License](LICENSE)
