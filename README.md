# prompt-ferry

An OpenAI-compatible AI gateway with peer-connected nodes. One node serves the browser, admin API, AI API and peer endpoint on one listener (default `:8787`). A node with PostgreSQL executes requests; a database-less edge forwards browser and AI traffic over its authenticated peer connection, without connecting to PostgreSQL or Valkey. Either side can list the other in `PROMPT_FERRY__PEER_URLS`; there is no multi-hop routing.

## Quick start

For an existing installation, **follow [MIGRATING.md](MIGRATING.md)** first. For a new Compose stack:

1. Copy `.env.example` to `.env`. Generate `PROMPT_FERRY__MASTER_SECRET` once with `openssl rand -base64 32`, keep it private and back it up. Fill the database and object-store credentials. The host database URL and `POSTGRES_PASSWORD` contain separate copies of the password; change both together.
2. Run `docker compose up -d`. Create the raw payload bucket in the RustFS console if raw storage is needed. The Compose example binds the node and service ports to loopback and keeps both data and config volumes. Do not discard them: the data root contains the persistent node identity and old config volumes may contain backups.
3. Visit `http://127.0.0.1:8787`. Complete first setup using the master secret over a trusted local connection or HTTPS, then log in normally. Create a managed AI client key in the UI and use that as the API bearer; the master secret is **not** an AI bearer token.

Set `PROMPT_FERRY__DATABASE_URL` to a PostgreSQL URL for a database node or leave it empty for an edge. Set peers as a JSON array, for example `["wss://node.example:8787"]`, or `[]`. All peers must run the same compiled version and protocol and share the same master secret. Nodes using the same database share state; multiple database nodes in a group require shared Valkey. A single database node without Valkey uses local sessions, which log out on restart; a configured but unavailable Valkey does not fall back. No cross-group in-memory takeover or transparent retry is promised once generation starts.

Use a canonical public HTTPS origin for MCP OAuth callbacks (`PROMPT_FERRY__MCP_OAUTH_REDIRECT_BASE`). Browser login is shared on the same origin; different domains require separate logins. Reverse proxies must preserve the original Host for setup Origin checks. See [SECURITY.md](SECURITY.md) for trust, TLS and proxy requirements.

## Development

Configure a stable master in the environment or a separate dotenv selected by `PROMPT_FERRY_DOTENV`, then run `bash scripts/dev.sh backend` or `bash scripts/dev.sh full`. Full mode uses plaintext on the default `127.0.0.1:8787` node bind and Vite's API proxy; use it only on a trusted development host. The scripts do not generate a master. Run backend unit checks with `bash scripts/ci/run-tests.sh`; see [CONTRIBUTING.md](CONTRIBUTING.md).
