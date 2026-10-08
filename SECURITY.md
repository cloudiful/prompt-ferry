# Security Policy

## Reporting a Vulnerability

Please report suspected vulnerabilities through GitHub Security Advisories for
this repository. Do not open a public issue or include credentials, API keys,
database URLs, request bodies, or other sensitive data in a report.

Include the affected version, deployment mode, reproduction steps, and the
smallest safe proof needed to validate the issue. Redact provider keys and
private hostnames before submitting logs.

Reports will be reviewed privately and handled according to the severity and
the currently supported release line.

## Deployment Guidance

- Keep the relay worker WebSocket and worker admin port on private networks.
- Keep the relay management listener (`relay.admin_bind`) on loopback. It serves
  the relay's own control API and must never be published on a routable address;
  reach it through a port forward or an SSH tunnel.
- The relay management token (`relay.admin_token`) is distinct from the relay
  client token, the `/ws/worker` worker token, and any worker admin login. Use a
  strong, unique value for it, or let the relay generate one and keep the
  generated host-local configuration file owner-only.
- A host-local service role (`integrated`, `worker`, or `relay`) decides only what
  this machine runs. It is stored in the host-local overlay beside the main
  configuration and never in shared managed-relay configuration or the business
  database; a saved role takes effect on the next restart.
- Use strong, unique values for all tokens and bootstrap passwords.
- Protect `PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY` like a database key.
- Use HTTPS or mutual TLS when traffic crosses a network boundary.
- Treat raw request and response logging as sensitive data.
- Back up PostgreSQL before upgrades and destructive migrations.
