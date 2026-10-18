import { expect, test } from 'bun:test'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

test('billing actions wrap instead of clipping at narrow widths', async () => {
  const page = await source('../src/pages/BillingPage.vue')

  expect(page).toContain('flex min-w-0 flex-1 flex-wrap justify-end gap-1.5')
  expect(page.match(/<UButton/g)?.length).toBeGreaterThanOrEqual(4)
})

test('billing filters carry visible labels and wrap', async () => {
  const filters = await source('../src/components/billing/BillingFilters.vue')

  expect(filters).toContain(':label="t(\'billingPeriodStart\')"')
  expect(filters).toContain(':label="t(\'billingPeriodEnd\')"')
  expect(filters).toContain("{{ t('billingPeriodTo') }}")
  expect(filters).toContain(':label="t(\'billingClientKeyId\')"')
  expect(filters).toContain(':label="t(\'publicModel\')"')
  expect(filters).toContain(':label="t(\'billingFilterUser\')"')
  expect(filters).toContain(':label="t(\'billingFilterEndpoint\')"')
  expect(filters).toContain(':label="t(\'billingFilterUsage\')"')
  expect(filters).toContain(':label="t(\'billingFilterPricing\')"')
  expect(filters).toContain('min-w-0')
  expect(filters).toContain('flex-wrap')
})

test('the charges table scrolls inside its own region', async () => {
  const panel = await source(
    '../src/components/billing/BillingChargesPanel.vue',
  )

  // Nuxt UI's table root is the scroll container; the minimum width belongs to
  // the table itself or the page overflows instead of the table scrolling.
  expect(panel).toContain("root: 'max-w-full overscroll-x-contain'")
  expect(panel).toContain("base: 'min-w-[60rem]'")
  expect(panel).not.toContain('class="min-w-[60rem]"')
})
