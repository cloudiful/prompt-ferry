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
- Use strong, unique values for all tokens and bootstrap passwords.
- Protect `PROMPT_FERRY_WORKER__RELAY_SECRET_MASTER_KEY` like a database key.
- Use HTTPS or mutual TLS when traffic crosses a network boundary.
- Treat raw request and response logging as sensitive data.
- Back up PostgreSQL before upgrades and destructive migrations.
