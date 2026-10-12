import { expect, test } from 'bun:test'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

test('perspective pending feedback travels from the store to the selector', async () => {
  const [page, category, workspace, overview, store] = await Promise.all([
    source('../src/pages/UsagePage.vue'),
    source('../src/components/usage/UsageCategoryPanel.vue'),
    source('../src/components/usage/UsageCategoryWorkspace.vue'),
    source('../src/components/RequestOverviewPanel.vue'),
    source('../src/stores/usage-actions.ts'),
  ])

  expect(page).toContain(
    ':perspective-pending="requestRecordsStore.perspectivePending"',
  )
  expect(category).toContain(':perspective-pending="perspectivePending"')
  expect(workspace).toContain(':perspective-pending="perspectivePending"')
  expect(overview).toContain(':disabled="perspectivePending"')
  expect(overview).toContain("category === 'ai' && perspectivePending")
  expect(store).toContain("'usage:perspective'")
})

test('clear dialog controls freeze for maintenance and spin only for clear', async () => {
  const [page, dialog, store] = await Promise.all([
    source('../src/pages/UsagePage.vue'),
    source('../src/components/usage/UsageClearDialog.vue'),
    source('../src/stores/request-records-actions.ts'),
  ])

  expect(page).toContain(':busy="requestRecordsStore.maintenancePending"')
  expect(page).toContain("requestRecordsStore.maintenanceAction === 'clear'")
  expect(dialog).toContain(':dismissible="!busy"')
  expect(dialog).toContain(':close="!busy"')
  expect(dialog).toContain(':loading="clearPending"')
  expect(dialog).toContain(':disabled="busy"')
  expect(store).toContain("'usage:maintenance'")
})

test('session-routing writes disable dismissal and spin only on the owner action', async () => {
  const [pageActions, usage, dialog, section, store, actionFactory] =
    await Promise.all([
      source('../src/composables/useSessionRoutingActions.ts'),
      source('../src/components/usage/UsagePanel.vue'),
      source('../src/components/usage/detail/UsageDetailDialog.vue'),
      source('../src/components/usage/detail/SessionRoutingSection.vue'),
      source('../src/stores/request-record-detail.ts'),
      source('../src/stores/request-record-detail-actions.ts'),
    ])

  expect(pageActions.match(/if \(outcome\.executed\)/g)?.length).toBe(2)
  expect(pageActions).toContain('if (!outcome.executed) return')
  expect(usage).toContain(':conversation-action="conversationAction"')
  expect(dialog).toContain(':dismissible="!writePending"')
  expect(dialog).toContain(':close="!writePending"')
  expect(section).toContain(':loading="conversationAction === \'save\'"')
  expect(section).toContain(':loading="conversationAction === \'clear\'"')
  expect(section).toContain(':loading="conversationAction === \'reset\'"')
  expect(section.match(/:disabled="sessionRouteActionsBusy"/g)?.length).toBe(3)
  expect(store).toContain('createRequestRecordDetailActions')
  expect(actionFactory).toContain(
    'recordActionKey(CONVERSATION_ACTION_PREFIX, conversationId)',
  )
})
