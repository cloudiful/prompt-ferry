# MiniMax Usage Visual Refinement

Status: COMPLETE

## Goal

根据最新套餐用量弹窗截图继续精简视觉：移除重复的“剩余”标签，让倒计时直接表达“还有多久重置”；让已使用进度颜色随使用比例连续从绿色过渡到黄色再到红色。

## Background

实现位于 `frontend/src/components/endpoints/TokenPlanUsageDialog.vue`，文案位于 `frontend/src/i18n/modules/endpoints.ts`。当前 key 已经按整行展示，窗口内容已移除卡片背景，但倒计时行仍显示“剩余”加“还有……重置”，进度条颜色仍只有 success/warning/error 三档。

## Constraints

- 只使用现有 Vue 3、Nuxt UI、Tailwind 和 i18n 模式。
- 不修改后端、OpenAPI 或 generated 类型。
- 本轮只允许修改：
  - `frontend/src/components/endpoints/TokenPlanUsageDialog.vue`
  - `frontend/src/i18n/modules/endpoints.ts`
- 任务开始时以下路径是用户基线，禁止修改、恢复、暂存或提交：
  - `src/cli.rs`
  - `src/config/types/worker.rs`
  - `src/runtime_env.rs`
  - `src/worker/runtime/bootstrap.rs`
  - `src/worker/runtime/connect/mod.rs`
- 不做无关格式化，不修改 lockfile，不创建新依赖。

## Acceptance Criteria

- 每个 quota 行不再显示重复的“剩余”标签；倒计时文案直接显示“还有 X 小时 Y 分钟重置”，缺失时间和到期状态仍清晰。
- 进度条仍表示已使用百分比，颜色随已使用比例连续变化：低使用量为绿色，中段为黄色，高使用量为红色；异常百分比被限制在 0 到 100。
- 保持 key 单行布局、简化边框和现有 loading、失败 key、无数据、缺失 window 状态。
- zh-CN 和 en-US 文案 key 保持一致，倒计时动态刷新行为保持不变。
- 目标文件通过格式检查，前端 typecheck 和 build 通过，`git diff --check` 通过。

## Phases

1. Visual refinement: DONE. 修改两个允许路径，移除重复“剩余”文案并实现连续 HSL 进度颜色。
2. Review and checkpoint: DONE. 独立审查 PASS；无确认的 P0-P2；已创建只包含两个允许路径和本计划归档的检查点提交。

## Validation

- `bunx prettier --check src/components/endpoints/TokenPlanUsageDialog.vue src/i18n/modules/endpoints.ts`: PASS
- `bun run typecheck`: PASS
- `bun run build`: PASS
- `git diff --check`: PASS
- Reviewer additionally confirmed the compiled scoped selector matches Nuxt UI's `data-slot="indicator"` and overrides the default indicator background.

## Dependencies

无；继续使用 `TokenPlanWindowUsage.remaining_percent`、现有倒计时状态和 Nuxt UI。

## Decisions

- Tracking: `LOCAL_PLAN`; tracking artifact: `.opencode/plans/archive/minimax-usage-visual-refinement.md`。
- 用已使用比例计算颜色，使用 HSL hue 120 到 0 的连续映射，避免仅在几个阈值跳色。
- 保持现有倒计时以 `end_at` 为主、`remains_time_ms` 为回退。
- Reviewer P3 notes accepted as non-blocking: color does not animate between updates, and fixed HSL values do not adapt to theme tokens.

## Baseline

- HEAD: `6487c94 chore(cleanup): remove one-time backfill tooling`
- Exact baseline paths: `src/cli.rs`, `src/config/types/worker.rs`, `src/runtime_env.rs`, `src/worker/runtime/bootstrap.rs`, `src/worker/runtime/connect/mod.rs`。
- Those baseline paths remain untouched and uncommitted.

## Review History

- Visual refinement executor: DONE. Changed only the two allowed frontend files. Targeted Prettier, typecheck, build, and diff checks passed.
- Independent reviewer: PASS. No P0-P2 findings; compiled CSS selector and all acceptance criteria verified. P3 notes accepted.

## Blocked Questions

无。

## Final Status

COMPLETE; user chose not to create a checkpoint after two pre-commit command typos. Staging was cleared each time without changing worktree content. The two frontend source changes remain uncommitted by request.
