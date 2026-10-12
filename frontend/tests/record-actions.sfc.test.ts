import { expect, test } from 'bun:test'

/**
 * The binding contract: how the pending state reaches the surfaces.
 *
 * These dialogs and rows are thin bindings over the store's pending map, and
 * the repo has no component-mount setup, so their contract is pinned against
 * the SFC source — the same pattern the endpoints surfaces use.
 */

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

const PENDING_KEYS = 'pendingKeys?: Readonly<Record<string, string>>'

test('a pending write freezes a dialog and every way out of it', async () => {
  for (const dialog of [
    '../src/components/users/CreateUserDialog.vue',
    '../src/components/users/ResetPasswordDialog.vue',
    '../src/components/users/KeyDialog.vue',
    '../src/components/mcp/McpDialog.vue',
  ]) {
    const text = await source(dialog)
    for (const binding of [
      ':dismissible="!savePending"',
      ':close="!savePending"',
      ':loading="busy || savePending"',
    ]) {
      expect(text).toContain(binding)
    }
    // The save is emitted by the native form, not awaited by the dialog, so
    // the dialog can only freeze itself — never resolve the parent's promise.
    expect(text).toContain('@submit.prevent="$emit(')
    expect(text).not.toContain('await $emit(')
  }
})

test('a pending write freezes the dialog body as a whole', async () => {
  for (const dialog of [
    '../src/components/users/CreateUserDialog.vue',
    '../src/components/users/ResetPasswordDialog.vue',
    '../src/components/mcp/McpDialog.vue',
  ]) {
    expect(await source(dialog)).toContain('<fieldset :disabled="savePending"')
  }
})

test('the draft editors inherit the freeze instead of faking a spinner', async () => {
  // The bearer-token and environment editors only edit a local draft, so they
  // must carry no loading binding at all; the dialog's fieldset is what
  // disables them while the parent write is in flight.
  for (const editor of [
    '../src/components/mcp/McpBearerTokensEditor.vue',
    '../src/components/mcp/McpEnvironmentEditor.vue',
  ]) {
    const text = await source(editor)
    expect(text).not.toContain(':loading=')
    // A nested form would defeat the inherited fieldset disable.
    expect(text).not.toContain('<form')
  }
})

test('the user rows gate on the record and spin only their own action', async () => {
  const text = await source('../src/components/users/UsersPanel.vue')

  expect(text).toContain(PENDING_KEYS)
  for (const action of ['reset', 'save', 'delete']) {
    expect(text).toContain(`pendingAction(item.user.user_id, '${action}')`)
    expect(text).toContain(
      `pendingAction(row.original.user.user_id, '${action}')`,
    )
  }
  // No row control may spin because the page happens to be fetching.
  expect(text).not.toContain('loading="workspace.busy"')
  expect(
    text.match(/isPending\(\s*(?:item|row\.original)\.user\.user_id\s*\)/g),
  ).toHaveLength(7)
})

test('the mcp rows gate on the record and keep only the table fetching', async () => {
  const text = await source('../src/components/mcp/McpServerListPanel.vue')

  expect(text).toContain(PENDING_KEYS)
  expect(text).toContain(
    `:loading="pendingAction(row.original.server.server_id, 'delete')"`,
  )
  expect(text).toContain(':disabled="isPending(row.original.server.server_id)"')
  // The only remaining `busy` binding is the table's own fetch indicator.
  expect(text.match(/:loading="busy"/g)).toHaveLength(1)
})

test('the client-key rows gate their own key, desktop and mobile alike', async () => {
  const text = await source('../src/components/users/ApiKeysList.vue')

  expect(text).toContain(PENDING_KEYS)
  expect(text).toContain(`pendingAction(keyItem.key_id, 'delete')`)
  expect(text).toContain(`pendingAction(row.original.key_id, 'delete')`)
  // The label input and the switch are the write controls of that row.
  expect(text.match(/:disabled="isPending\(/g)).toHaveLength(4)
})

test('the pages hand the pending state to every surface', async () => {
  const users = await source('../src/pages/UsersPage.vue')
  expect(users).toContain(':pending-keys="userPendingActions"')
  expect(users).toContain(':save-pending="createUserPending"')
  expect(users).toContain(':save-pending="resetPasswordPending"')

  const keys = await source('../src/pages/ApiKeysPage.vue')
  expect(keys).toContain(':pending-keys="apiKeysStore.clientKeyPendingActions"')
  expect(keys).toContain(':save-pending="createKeyPending"')

  const mcp = await source('../src/pages/McpPage.vue')
  expect(mcp).toContain(':save-pending="mcpSavePending"')
})

test('the mcp row state survives both hops from page to rows', async () => {
  // The map travels page -> panel -> list. Each hop has to name the prop, or
  // the rows gate on nothing and the whole MCP binding is silently dead.
  const page = await source('../src/pages/McpPage.vue')
  expect(page).toContain(':pending-keys="mcpStore.mcpPendingActions"')

  const panel = await source('../src/components/mcp/McpPanel.vue')
  expect(panel).toContain('pendingKeys?: Readonly<Record<string, string>>')
  // The panel is a pass-through: it forwards the map and invents nothing.
  expect(panel).toContain(':pending-keys="pendingKeys"')

  const list = await source('../src/components/mcp/McpServerListPanel.vue')
  expect(list).toContain('pendingKeys?: Readonly<Record<string, string>>')
  // The panel is the only thing between the page and the rows, and it must
  // forward rather than transform.
  expect(panel.match(/:pending-keys="pendingKeys"/g)).toHaveLength(1)
})
