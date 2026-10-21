import { expect, test } from 'bun:test'
import { parse } from 'vue/compiler-sfc'

const pageSource = await Bun.file(
  new URL(
    '../src/components/endpoints/ModelRouteTargetSettingsPage.vue',
    import.meta.url,
  ),
).text()

const template = parse(pageSource, {
  filename: 'ModelRouteTargetSettingsPage.vue',
}).descriptor.template!.content

// Issue #1004 P1: the reasoning selector scrolls only when the available
// viewport space is insufficient. The select content is portalled, so the
// binding is asserted on the component source.
test('the reasoning select follows the measured available height', () => {
  expect(template).toContain('--reka-select-content-available-height')
  expect(template).toContain('max-h-[min(')
  // No replacement fixed cap: the content is bounded by the measured
  // viewport space, not by a hardcoded rem height.
  expect(template).not.toContain('15rem')
  expect(template).not.toContain('28rem')
})

test('the reasoning select keeps its own content binding', () => {
  // The binding is per instance, so the global select theme stays untouched.
  const selectBlocks = template.split('<USelect')
  const reasoning = selectBlocks.find((block) =>
    block.includes('thinkingEffortInherit'),
  )
  expect(reasoning).toBeDefined()
  expect(reasoning).toContain(':ui=')
  expect(reasoning).toContain('content:')
})
