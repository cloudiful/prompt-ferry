import { expect, test } from 'bun:test'

const source = (path: string) => Bun.file(new URL(path, import.meta.url)).text()

test('relay row and dialog controls bind the owner pending action end to end', async () => {
  const [page, actions, dialog] = await Promise.all([
    source('../src/pages/RelaysPage.vue'),
    source('../src/composables/useRelaysPageActions.ts'),
    source('../src/components/relays/RelayDialog.vue'),
  ])

  expect(actions).toContain('relayStore.relayPendingActions[')
  expect(actions).toContain('dialogForm.value.relay_id || CREATE_KEY')
  expect(actions).toContain('dialogPendingAction.value !== undefined')
  expect(actions).toContain("dialogPendingAction.value === 'save'")
  expect(page).toContain(':pending="dialogPending"')
  expect(page).toContain(':save-pending="dialogSavePending"')
  expect(page).toContain(
    'relayStore.relayPendingActions[row.original.relay_id]',
  )
  expect(page).toContain("'reconnect'")
  expect(page).toContain("'delete'")
  expect(page).toContain(':disabled="createPending"')

  expect(dialog).toContain(':dismissible="!pending"')
  expect(dialog).toContain(':close="!pending"')
  expect(dialog).toContain(':disabled="pending"')
  expect(dialog).toContain(':loading="savePending"')
  for (const component of ['UInput', 'USelect', 'UCheckbox', 'UTextarea']) {
    const controls =
      dialog.match(new RegExp(`<${component}\\b[^>]*>`, 'g')) ?? []
    expect(controls.length).toBeGreaterThan(0)
    expect(
      controls.every((control) => control.includes(':disabled="pending"')),
    ).toBe(true)
  }
  expect(dialog.match(/:disabled="pending"/g)?.length).toBeGreaterThan(2)
})

test('Relay Host pages and components freeze the shared host resource', async () => {
  const [page, actions, roles, banner] = await Promise.all([
    source('../src/pages/RelayHostPage.vue'),
    source('../src/composables/useRelayHostPageActions.ts'),
    source('../src/components/relays/HostRoleCards.vue'),
    source('../src/components/relays/RestartRequiredBanner.vue'),
  ])

  expect(actions).toContain('hostStore.relayHostPendingActions.local')
  expect(page).toContain(':pending-role-action="roleActionTarget"')
  expect(page).toContain(':write-pending="hostWritePending"')
  expect(page).toContain(':pending="hostWritePending"')
  expect(page).toContain(':disabled="hostWritePending"')
  expect(page).toContain("hostPendingAction === 'settings'")
  expect(page).toContain(':busy="bannerRestartPending"')
  expect(page).toContain(':loading="settingsRestartPending"')
  expect(page).toContain("requestRestart('banner')")
  expect(page).toContain("requestRestart('settings')")
  expect(actions).toContain('restartOrigin')
  expect(page).toContain("hostPendingAction === 'role'")

  expect(roles).toContain(':loading="busy && pendingRoleAction === item.role"')
  expect(roles).toContain(
    ':disabled="writePending || currentRole === item.role"',
  )
  expect(banner).toContain(':loading="busy"')
  expect(banner).toContain(':disabled="pending"')

  // Refresh remains a read-only page action, separate from the host write key.
  expect(page).toContain(':loading="hostStore.loading || bootstrapping"')
})
