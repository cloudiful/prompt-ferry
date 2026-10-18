import { expect, test } from 'bun:test'

const source = () =>
  Bun.file(new URL('../src/layouts/AppShell.vue', import.meta.url)).text()

test('the shared shell exposes a small-screen navigation control', async () => {
  const shell = await source()

  expect(shell).toContain('v-model:open="mobileNavOpen"')
  expect(shell).toContain(
    '<UDashboardNavbar class="lg:hidden" :toggle="false">',
  )
  expect(shell).toContain(
    '<UDashboardSidebarToggle :aria-label="t(\'expandNav\')" />',
  )
  // The mobile toggle must render outside the collapsed-on-desktop branch.
  expect(shell.indexOf('mobileNavOpen')).toBeLessThan(
    shell.indexOf('v-if="collapsed"'),
  )
})

test('desktop collapse behaviour stays unchanged', async () => {
  const shell = await source()

  expect(shell).toContain('v-model:collapsed="collapsed"')
  expect(shell).toContain('<UDashboardSidebarCollapse />')
  expect(shell).toContain(':collapsed="collapsed"')
})
