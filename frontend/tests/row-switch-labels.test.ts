import { expect, test } from 'bun:test'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

const section = (text: string, start: string, end: string) => {
  const from = text.indexOf(start)
  const to = text.indexOf(end, from + start.length)
  expect(from).toBeGreaterThan(-1)
  expect(to).toBeGreaterThan(from)
  return text.slice(from, to)
}

test('endpoint switches name the endpoint they control', async () => {
  const table = await source('../src/components/endpoints/EndpointsTable.vue')
  const statusCell = section(table, '#status-cell', '#usage-cell')

  expect(statusCell).toContain("t('endpointEnabledToggle'")
  expect(statusCell).toContain('row.original.name')
  expect(statusCell).not.toContain("t('status')")

  const card = await source(
    '../src/components/endpoints/EndpointMobileCard.vue',
  )
  expect(card).toContain(
    ':aria-label="t(\'endpointEnabledToggle\', { name: item.name })"',
  )
})

test('user row switches carry the account identity', async () => {
  const users = await source('../src/components/users/UsersPanel.vue')

  const mobile = section(users, 'md:hidden', '<UTable')
  expect(mobile).toContain("t('userAdminToggle'")
  expect(mobile).toContain("t('userActiveToggle'")
  expect(mobile).toContain('item.login_name')
  // The mobile card has no column headers, so its visible labels stay.
  expect(mobile).toContain("{{ t('adminShort') }}")
  expect(mobile).toContain("{{ t('active') }}")

  const desktop = section(users, '<UTable', '</UTable>')
  const adminCell = section(desktop, '#admin-cell', '#active-cell')
  const activeCell = section(desktop, '#active-cell', '#actions-cell')

  for (const [cell, key] of [
    [adminCell, 'userAdminToggle'],
    [activeCell, 'userActiveToggle'],
  ] as const) {
    expect(cell).toContain(`t('${key}'`)
    expect(cell).toContain('row.original.login_name')
  }
  // The header already names these columns, so the cells do not repeat them.
  expect(adminCell).not.toContain("t('adminShort')")
  expect(activeCell).not.toContain("t('active')")
})
