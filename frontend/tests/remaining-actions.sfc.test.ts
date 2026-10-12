import { expect, test } from 'bun:test'

/**
 * The binding contract for the remaining record domains.
 *
 * These surfaces are thin bindings over a store's pending map, and the repo has
 * no component-mount setup, so the contract is pinned against the SFC source —
 * the same pattern the endpoints and record-action surfaces use.
 */

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

test('approval actions share pending UI across layouts', async () => {
  const [panel, buttons] = await Promise.all([
    source('../src/components/approvals/ApprovalsPanel.vue'),
    source('../src/components/approvals/ApprovalActionButtons.vue'),
  ])

  expect(panel).toContain('pendingKeys?: Readonly<Record<string, string>>')
  expect(panel).toContain('layout="mobile"')
  expect(panel).toContain('layout="desktop"')
  expect(panel.match(/:pending-keys="pendingKeys"/g)?.length).toBe(2)
  // Both layouts route through the same per-approval gate and only the action
  // that claimed it spins; the other button stays disabled.
  expect(buttons).toContain('function isPending()')
  expect(buttons).toContain("pendingAction('approve')")
  expect(buttons).toContain("pendingAction('reject')")
  expect(buttons.match(/:disabled="isPending\(\)"/g)?.length).toBe(2)
  expect(buttons).toContain("layout === 'mobile' ? 'outline' : 'ghost'")
  expect(buttons).toContain(`layout === 'mobile'">{{ t('reject') }}`)
  expect(buttons).toContain(`layout === 'mobile'">{{ t('approve') }}`)
  // The only remaining `busy` bindings are the table fetch indicator and the
  // detail pane.
  expect(panel.match(/:loading="busy"/g)?.length ?? 0).toBeLessThanOrEqual(2)
})

test('the approvals page hands the pending map to the panel', async () => {
  const text = await source('../src/pages/ApprovalsPage.vue')

  expect(text).toContain(
    ':pending-keys="approvalsStore.approvalPendingActions"',
  )
})

test('the approvals page completes only an answer that ran', async () => {
  const text = await source('../src/pages/ApprovalsPage.vue')

  // A refused call is the guard collapsing a duplicate trigger: no toast.
  expect(text).toContain(
    "if (result.executed) notifySuccess(t('approvalApproved'))",
  )
  expect(text).toContain(
    "if (result.executed) notifySuccess(t('approvalRejected'))",
  )
})

/**
 * The settings surfaces.
 *
 * Every settings write goes through a section key, so the surfaces' only job is
 * to report a save that ran and to spin on their own section rather than on the
 * whole page.
 */

test('the settings page delegates its saves to the page-actions composable', async () => {
  const text = await source('../src/pages/SettingsPage.vue')

  expect(text).toContain(
    'useSettingsPageActions({ activeSection, store: settingsStore, t })',
  )
  // The autosave machinery is the composable's, not the page's.
  expect(text).not.toContain('createAutosaveController')
  expect(text).not.toContain('requestAutosaveReady')
})

test('a settings surface reports only a save that reached the server', async () => {
  const [section, storage] = await Promise.all([
    source('../src/components/settings/SettingsCacheAlertSection.vue'),
    source('../src/components/settings/SettingsStorageTab.vue'),
  ])

  for (const text of [section, storage]) {
    expect(text).toContain('outcome.executed')
    expect(text).toContain('notifySuccess')
  }
  // Storage used to carry its own `saving` flag, which is the guard's job and
  // would let a second click read as "not busy".
  expect(storage).not.toMatch(/\bsaving\b/)
})

test('each settings surface spins on its own section, not on the page', async () => {
  const [section, storage] = await Promise.all([
    source('../src/components/settings/SettingsCacheAlertSection.vue'),
    source('../src/components/settings/SettingsStorageTab.vue'),
  ])

  expect(section).toContain('settingsPendingActions.cacheAlert')
  expect(storage).toContain('settingsPendingActions.storage')
})

test('redaction save and preview report independent owner-level pending state', async () => {
  const [page, store] = await Promise.all([
    source('../src/pages/RedactionPage.vue'),
    source('../src/stores/redaction-actions.ts'),
  ])

  expect(page).toContain("if (outcome.executed) notifySuccess(t('saved'))")
  expect(page).toContain(':loading="redactionStore.saving"')
  expect(page).toContain(':busy="redactionStore.previewing"')
  expect(store).toContain("'redaction:rules'")
  expect(store).toContain("'redaction:preview'")
  expect(store).toContain('capturePayload(config.value)')
})

test('config archive completion is reported only after an accepted action', async () => {
  const [actions, store, card] = await Promise.all([
    source('../src/composables/useConfigArchiveActions.ts'),
    source('../src/stores/settings/config-archive.ts'),
    source('../src/components/settings/ConfigArchiveImportCard.vue'),
  ])

  expect(actions).toContain('if (!outcome.executed) return false')
  expect(actions).toContain('onSuccess?.()')
  expect(store).toContain("'configArchive:import'")
  expect(store).toContain('await refreshConfigAudit()')
  expect(card).toContain(':busy="store.importPending"')
  expect(card).toContain(':disabled="store.importPending"')
})

test('each settings surface is one script block closed by its template', async () => {
  const surfaces = [
    '../src/pages/SettingsPage.vue',
    '../src/components/settings/SettingsCacheAlertSection.vue',
    '../src/components/settings/SettingsStorageTab.vue',
    '../src/components/settings/SettingsNetworkTab.vue',
    '../src/components/settings/SettingsReviewTab.vue',
  ]

  for (const relative of surfaces) {
    const text = await source(relative)
    // `tsgo` does not parse SFCs, so a dropped `</script>` is otherwise
    // invisible until the page is built.
    expect(text.match(/<\/script>/g)?.length ?? 0).toBe(1)
    expect(text.indexOf('</script>')).toBeLessThan(text.indexOf('<template>'))
  }
})

test('a button-saved settings tab spins on its own section, not the page', async () => {
  const [network, review] = await Promise.all([
    source('../src/components/settings/SettingsNetworkTab.vue'),
    source('../src/components/settings/SettingsReviewTab.vue'),
  ])

  for (const text of [network, review]) {
    expect(text).toContain('pending?: boolean')
    // The page refresh still disables the button; only the section's own save
    // adds a spinner, and neither may spin because the page happens to be busy.
    expect(text.match(/:loading="pending \|\| busy"/g)?.length ?? 0).toBe(1)
    expect(text).not.toContain(':loading="busy"')
  }
})

test('the settings page hands each button-saved tab its own section', async () => {
  const text = await source('../src/pages/SettingsPage.vue')

  expect(text).toContain(':pending="relayIpWhitelistPending"')
  expect(text).toContain('settingsPendingActions.relayIpWhitelist')
  expect(text).toContain(':pending="llmReviewPending"')
  expect(text).toContain('settingsPendingActions.llmReview')
})
