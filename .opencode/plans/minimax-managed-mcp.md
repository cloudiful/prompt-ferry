# Managed MiniMax MCP

## Goal

Expose MiniMax Coding Plan `web_search` and `understand_image` tools as a managed MCP projection of an existing MiniMax provider endpoint, while preserving generic HTTP/stdio MCP support and endpoint key reuse.

## Background

The third-party MiniMax stdio package is incompatible with the installed MCP Python SDK. The implementation replaces that path for MiniMax with a Rust builtin transport, binds the MCP row to `provider_endpoints`, and adds admin/frontend controls for the projection. The change also includes the requested MCP diagnostics and billing token normalization fix.

## Constraints

- No duplicated MiniMax secrets: builtin calls use the bound endpoint's existing key pool.
- `builtin_minimax` rows must have a valid MiniMax `source_endpoint_id`; HTTP/stdio rows must not retain one.
- Image URL fetches must be HTTPS-only, DNS-pinned after validation, redirect-free, size-limited, and protected from private/reserved address ranges.
- Static SQL belongs in `.sql` files and is loaded through SQLx query-file macros.
- Do not stage `Cargo.lock` or `bun.lock`.
- Production migration and service restart are operational steps, not performed automatically.

## Acceptance Criteria

- Existing MiniMax endpoint create/update can enable or disable a managed MCP projection without manually configuring a second secret.
- Managed MCP catalog exposes only `web_search` and `understand_image`, honoring tool filters.
- Builtin calls select enabled endpoint keys, use the correct MiniMax region/API headers, return useful upstream errors, and record usage token slots.
- `understand_image` rejects unsafe image sources and does not follow redirects.
- Admin API/OpenAPI/frontend models and forms represent `mcp_enabled` and `source_endpoint_id` consistently.
- Provider changes away from MiniMax clear MCP exposure state and managed cache entries are invalidated on endpoint deletion.
- Migration 0064 is idempotent; migration 0065 clamps historical negative token values before enforcing non-negative counts.
- Generic stdio MCP UX supports JSON-array command input, worker/value environment sources, secret masking, and useful MCP test logs.

## Baseline

- HEAD at continuation checkpoint: `2af88ce129049ea1c9d3fffcb79eec4847cf89fc`.
- No staged paths were present.
- The following unstaged/untracked paths were the task worktree carried from the prior implementation context and are treated as task-owned for this checkpoint:

```text
frontend/src/admin-mappers/forms/endpoint.ts
frontend/src/admin-mappers/forms/mcp.ts
frontend/src/components/endpoints/EndpointDialog.vue
frontend/src/components/endpoints/EndpointProviderFields.vue
frontend/src/components/endpoints/EndpointsTable.vue
frontend/src/components/mcp/McpDialog.vue
frontend/src/generated/admin-api/types.gen.ts
frontend/src/i18n/modules/endpoints.ts
frontend/src/i18n/modules/mcp.ts
frontend/src/models.ts
frontend/src/models/endpoints.ts
frontend/src/models/endpoints/endpoint-item.ts
frontend/src/models/mcp.ts
frontend/src/stores/mcp.ts
openapi/admin-api.yaml
src/db.rs
src/db/endpoints.rs
src/db/mcp.rs
src/db/types/billing.rs
src/db/types/endpoints.rs
src/db/types/mcp.rs
src/db/usage/insert.rs
src/mcp/cache.rs
src/mcp/entry/tests.rs
src/mcp/filtering.rs
src/mcp/mod.rs
src/mcp/service/snapshot.rs
src/mcp/service/tests.rs
src/mcp/transport/client.rs
src/mcp/transport/mod.rs
src/mcp/transport/v2_tests.rs
src/sql/endpoints/create_endpoint.sql
src/sql/endpoints/get_endpoint.sql
src/sql/endpoints/list_endpoints.sql
src/sql/endpoints/list_endpoints_page.sql
src/sql/endpoints/update_endpoint.sql
src/sql/mcp/create_mcp_server.sql
src/sql/mcp/get_mcp_server.sql
src/sql/mcp/get_mcp_server_by_name.sql
src/sql/mcp/get_user_mcp_server.sql
src/sql/mcp/get_visible_mcp_server.sql
src/sql/mcp/list_mcp_servers.sql
src/sql/mcp/list_mcp_servers_page.sql
src/sql/mcp/list_user_mcp_servers.sql
src/sql/mcp/list_user_mcp_servers_page.sql
src/sql/mcp/list_visible_mcp_servers.sql
src/sql/mcp/update_mcp_server.sql
src/usage/capture.rs
src/usage/text.rs
src/worker_admin/handlers/endpoints.rs
src/worker_admin/handlers/mcp.rs
src/worker_admin/handlers/support.rs
src/worker_admin/types.rs
src/worker_admin/types/endpoints.rs
src/worker_admin/types/mcp.rs
tests/db_migrations.rs
migrations/0064_minimax_managed_mcp.up.sql
migrations/0065_usage_charge_token_counts_nonnegative.up.sql
src/mcp/builtin/
src/sql/endpoints/create_endpoint_with_mcp.sql
src/sql/endpoints/set_mcp_enabled.sql
src/sql/mcp/get_mcp_server_by_source_endpoint.sql
src/sql/mcp/update_managed_mcp_server.sql
```

## Phases

### 1. Implement managed MiniMax MCP and stdio UX

**In scope:** the baseline paths above. **Out of scope:** production database/service operations, lockfile churn, unrelated refactors.

Outcome: Rust builtin transport, endpoint projection/sync, migrations, admin API/OpenAPI/frontend support, stdio editor improvements, billing normalization, and diagnostics.

Validation:

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets`
- `cargo test --workspace`
- `bun run typecheck`
- `git diff --check`

Result: complete. Workspace tests passed with 694 tests and no failures; formatting, checks, and frontend typecheck passed.

### 2. Independent review and repairs

**In scope:** the same implementation paths plus the repair files `src/sql/endpoints/create_endpoint_with_mcp.sql` and the updated migration. **Out of scope:** unrelated baseline changes.

Outcome: resolve SSRF, source-binding, cache invalidation, migration safety, atomic endpoint-create, concurrency, and diagnostic-log findings.

Review history:

- Initial review found reachable image-fetch SSRF, managed-row desynchronization, provider state desynchronization, missing coverage, frontend source-binding issues, and raw error logging.
- First repair added DNS/IP validation and pinning, redirect disabling, managed-row recovery, provider/form cleanup, focused tests, and redacted logs.
- Second review found source-binding merge, endpoint-delete cache, migration clamp, create write-window, and concurrent sync issues.
- Final repair addressed all five issues.
- Final independent review: no actionable P0-P2 findings; overall correctness `patch is correct`.

Validation after repairs:

- `cargo fmt --all -- --check`: pass
- `cargo check --workspace --all-targets`: pass
- `cargo test --workspace`: pass, 694 tests
- `bun run typecheck`: pass
- `git diff --check`: pass

## Dependencies

- Production must apply migrations 0064 and 0065 before deploying code that reads/writes `provider_endpoints.mcp_enabled` or `mcp_servers.source_endpoint_id`.
- Worker/frontend images must be rebuilt and restarted after migration and code deployment.
- Existing manually configured MiniMax stdio rows need operator review; the managed projection is created from the MiniMax endpoint toggle and does not silently delete unrelated MCP rows.

## Decisions

- MiniMax MCP is a managed projection, not an independently configured MCP server.
- Endpoint API-key pools and their load-balancing settings are reused.
- MiniMax region selects `api.minimaxi.com` or `api.minimax.io`.
- The builtin adapter is transport-local and does not create an upstream MCP child process.
- Historical negative token counts are clamped before the new database constraint is added.

## Blocked Questions

- None for implementation.
- Operational confirmation remains required from the deployer before production migration/restart.

## Checkpoint

The implementation paths were already present as the continuation's baseline worktree state, so no source checkpoint commit was created. Creating a commit from this continuation would claim ownership of baseline edits. The plan file is the only new orchestrator-owned artifact.

## Final Status

COMPLETE FOR IMPLEMENTATION. Code, validation, and independent review are complete. Production migration and deployment remain pending.

## Residual Risks

- Existing production database has not yet been migrated in this session.
- Existing MiniMax stdio configuration may remain until an administrator removes or disables it after verifying the managed projection.
- External MiniMax API behavior is covered by request construction and error handling, but live API success depends on endpoint credentials, region, quota, and upstream availability.
