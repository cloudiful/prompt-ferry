import { expect, test } from 'bun:test'

/**
 * The binding contract: how the pending state reaches the surfaces.
 *
 * The dialogs and rows are thin bindings over that state, and the repo has no
 * component-mount setup, so their contract is pinned against the SFC source —
 * the save gate must reach the controls and every dismissal path, and the row
 * gate must reach the desktop and mobile surfaces alike.
 */

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

const dialogs = [
  '../src/components/endpoints/EndpointDialog.vue',
  '../src/components/endpoints/ModelRouteDialog.vue',
]

const desktop = [
  ['../src/components/endpoints/EndpointsTable.vue', 'endpoint_id'],
  ['../src/components/endpoints/ModelRoutesTable.vue', 'rule_id'],
] as const

const mobile = [
  '../src/components/endpoints/EndpointMobileCard.vue',
  '../src/components/endpoints/ModelRouteMobileCard.vue',
]

const tabs = [
  '../src/components/endpoints/EndpointsEndpointsTab.vue',
  '../src/components/endpoints/EndpointsRoutesTab.vue',
]

const PENDING_KEYS = 'pendingKeys?: Readonly<Record<string, string>>'

test('a pending save freezes the dialog controls and every way out of it', async () => {
  for (const dialog of dialogs) {
    const text = await source(dialog)
    for (const binding of [
      ':dismissible="!savePending"',
      ':close="!savePending"',
      '<fieldset :disabled="savePending"',
      ':disabled="!canSaveOuter || savePending"',
      ':disabled="savePending"',
    ]) {
      expect(text).toContain(binding)
    }
  }
})

test('save and OAuth spinners reflect their own pending action', async () => {
  const dialog = await source('../src/components/endpoints/EndpointDialog.vue')
  expect(dialog).toContain(
    ':loading="busy || (savePending && pendingAction === \'save\')"',
  )
  expect(dialog).not.toContain(':loading="busy || savePending"')
  expect(dialog).toContain('<fieldset :disabled="savePending"')
  expect(dialog).toContain(':dismissible="!savePending"')
  expect(dialog).toContain(':close="!savePending"')

  const oauth = await source(
    '../src/components/endpoints/EndpointOAuthSection.vue',
  )
  expect(oauth).toContain(':loading="refreshing"')
  expect(oauth).toContain(':loading="clearing"')
  expect(oauth).not.toContain(':loading="savePending"')

  const device = await source(
    '../src/components/endpoints/EndpointOAuthDeviceLogin.vue',
  )
  expect(device).toContain(':loading="starting"')

  const browser = await source(
    '../src/components/endpoints/EndpointOAuthBrowserLogin.vue',
  )
  expect(browser).toContain(':loading="busy && !browser"')
  expect(browser).toContain(':loading="busy && Boolean(redirectUrl.trim())"')
})

test('the desktop rows gate on the record and spin only their own action', async () => {
  for (const [row, id] of desktop) {
    const text = await source(row)
    expect(text).toContain(PENDING_KEYS)
    expect(text).toContain(
      `:loading="pendingAction(row.original.${id}) === 'toggle'"`,
    )
    expect(text).toContain(
      `:loading="pendingAction(row.original.${id}) === 'delete'"`,
    )
    expect(text).toContain(`:disabled="isPending(row.original.${id})"`)
    // The only page-busy binding left is the fetch indicator on the table
    // itself, so a delete no longer spins because the page happens to be
    // fetching.
    expect(text.match(/:loading="busy"/g) ?? []).toHaveLength(1)
  }
})

test('the mobile cards gate their own row the same way', async () => {
  for (const card of mobile) {
    const text = await source(card)
    expect(text).toContain(PENDING_KEYS)
    expect(text).toContain(':loading="pendingAction() === \'toggle\'"')
    expect(text).toContain(':loading="pendingAction() === \'delete\'"')
    expect(text).toContain(':disabled="isPending()"')
    expect(text).not.toContain(':loading="busy"')
  }
})

test('the page hands the pending state to both tabs and both dialogs', async () => {
  const text = await source('../src/pages/EndpointsPage.vue')

  for (const binding of [
    ':pending-keys="endpointPendingActions"',
    ':pending-keys="modelRoutePendingActions"',
    ':save-pending="endpointSavePending"',
    ':save-pending="modelRouteSavePending"',
  ]) {
    expect(text).toContain(binding)
  }
})

test('each tab forwards its pending map to the desktop and mobile rows', async () => {
  for (const tab of tabs) {
    const text = await source(tab)
    expect(text).toContain(PENDING_KEYS)
    // Once to the mobile row, once to the desktop table.
    expect(text.match(/:pending-keys="pendingKeys"/g)).toHaveLength(2)
  }
})
