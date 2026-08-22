# 页面说明信息密度优化

Status: COMPLETE

## Tracking

- Mode: LOCAL_PLAN
- Reason: Redmine project creation was attempted for `tools/prompt-ferry` and returned HTTP 403. Existing Redmine projects did not include this repository, and the available workflow CLI cannot create the project with the current credential.
- Redmine project request: name `tools/prompt-ferry`, identifier `tools-prompt-ferry`; no issue was created because the project prerequisite failed.

## Goal

Reduce persistent, low-value explanatory copy across the frontend. Keep task-critical warnings visible, move control-specific guidance into accessible `i-lucide-info` tooltips, and reclaim vertical space in the MCP editor and related settings pages.

## Background

The MCP editor currently renders several long hint paragraphs on every open. The same pattern appears in endpoint, quota, billing, settings, and usage dialogs. The repository already uses Nuxt UI `UTooltip` with an info icon for control-level guidance.

## Constraints

- Preserve behavior, API contracts, validation, and security-sensitive warnings.
- Use existing Nuxt UI and Lucide icon patterns; do not add dependencies.
- Keep English and Simplified Chinese translations aligned.
- Do not edit baseline paths recorded below.
- Do not modify unrelated user work in `/workspace/tools/agent-task`.

## Acceptance Criteria

- MCP editor no longer shows low-value explanatory paragraphs as permanent layout content.
- Control-specific guidance is available through an info icon tooltip with an accessible label.
- Equivalent always-visible hints on other pages are converted or removed consistently.
- Destructive or state-dependent warnings remain visible when relevant.
- Frontend typecheck, format check, and production build pass.
- The final review returns the exact token `VERDICT: PASS`.

## Baseline

- Repository: `/workspace/tools/prompt-ferry`
- HEAD: `b104124`
- Staged paths: none
- Unstaged paths: none
- Untracked paths: none

## Phases

1. `frontend-density-audit-and-plan`: identify visible copy and define the scoped component/i18n set. Outcome: confirmed implementation scope. **Complete**: audit found the MCP dialog as the highest-density area and existing tooltip patterns in endpoint/MCP components.
2. `mcp-hints`: convert MCP dialog, environment, credential, and quota hints to label-adjacent info tooltips; remove redundant empty-state/helper copy where safe. **Complete**: five MCP components changed; typecheck/build and phase-file format checks passed.
3. `cross-page-hints`: apply the same treatment to endpoint, billing, settings, usage, and other audited pages; keep warnings and meaningful empty states. **Complete**: seven cross-page components changed; typecheck/build and phase-file format checks passed.
4. `aggregate-review`: independently review the full committed change and resolve confirmed P0-P2 findings. **Complete**: round 1 returned exact `VERDICT: PASS` with P0/P1/P2 = 0 and three P3 polish findings; the narrow P3 cleanup passed round 2 review with exact `VERDICT: PASS` and P0-P3 = 0.

## Scope

In scope: `frontend/src/components/mcp/**`, the specific endpoint/billing/settings/usage components identified by the audit, and corresponding `frontend/src/i18n/modules/**` entries.

Out of scope: backend/API/schema changes, generated API clients, dependency changes, unrelated layout redesign, and `/workspace/tools/agent-task/**`.

## Validation

- `bun run typecheck` from `frontend`
- `bun run format:check` from `frontend`
- `bun run build` from `frontend`
- `git diff --check`

## Dependencies

- Nuxt UI `UTooltip` and existing `i-lucide-info` icon support.
- Existing locale keys and `useLocale` translation function.

## Decisions

- Use `LOCAL_PLAN` after Redmine project creation returned 403.
- Keep warnings and destructive-action explanations visible; only move contextual guidance.
- Prefer label-adjacent tooltip icons over free-floating helper paragraphs.

## Review History

- Aggregate review round 1: exact `VERDICT: PASS`; P0/P1/P2 = 0, P3 = 3. Findings were limited to tooltip separator presentation, one dead wrapper, and placing the billing-period tooltip on only one of two related fields.
- Aggregate review round 2 after P3 cleanup: exact `VERDICT: PASS`; P0/P1/P2/P3 = 0.

## Blocked Questions

- Redmine project creation requires a credential with project-create permission; current credential returned HTTP 403.

## Final Status

COMPLETE. Frontend behavior and validation passed. Redmine project creation remains externally blocked by HTTP 403 for the current credential; no Redmine issue was created.
