import { expect, test } from 'bun:test'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

test('settings page binds pending state to saved tabs', async () => {
  const page = await source('../src/pages/SettingsPage.vue')

  expect(page).toContain(
    '() => !!settingsStore.settingsPendingActions.relayIpWhitelist',
  )
  expect(page).toContain(
    '() => !!settingsStore.settingsPendingActions.llmReview',
  )
  expect(page).toContain(':pending="relayIpWhitelistPending"')
  expect(page).toContain(':pending="llmReviewPending"')
})

test('network and review controls show and disable their own accepted save', async () => {
  const [network, review, llmReview] = await Promise.all([
    source('../src/components/settings/SettingsNetworkTab.vue'),
    source('../src/components/settings/SettingsReviewTab.vue'),
    source('../src/components/settings/LlmReviewSettingsCard.vue'),
  ])

  expect(network.match(/:loading="pending \|\| busy"/g)?.length).toBe(1)
  expect(network.match(/:disabled="pending \|\| busy"/g)?.length).toBe(3)
  expect(review.match(/:loading="pending \|\| busy"/g)?.length).toBe(1)
  expect(review).toContain(':busy="busy || pending"')
  expect(llmReview).toContain('v-model="llmReview.review_base_url"')
  expect(llmReview.match(/:disabled="busy"/g)?.length).toBe(11)
})

test('retention forms share the one server resource key', async () => {
  const [actions, requests, page] = await Promise.all([
    source('../src/stores/settings-actions.ts'),
    source('../src/components/settings/SettingsRequestsTab.vue'),
    source('../src/pages/SettingsPage.vue'),
  ])

  expect(actions).toContain("requestContentLogging: 'rawRetention'")
  expect(actions).toContain("usageRetention: 'rawRetention'")
  expect(requests).toContain(
    'v-model="requestContentLogging.raw_retention_days"',
  )
  expect(page).toContain(
    'v-model:usage-retention="settingsStore.usageRetention"',
  )
})

test('whitelist and stream autosaves expose pending state to their own controls', async () => {
  const [page, requests] = await Promise.all([
    source('../src/pages/SettingsPage.vue'),
    source('../src/components/settings/SettingsRequestsTab.vue'),
  ])

  expect(page).toContain(
    "settingsStore.isPending(settingFormKey('modelRouteWhitelist'))",
  )
  expect(page).toContain(
    "settingsStore.isPending(settingFormKey('streamDeltaBatching'))",
  )
  expect(page).toContain(
    ':model-route-whitelist-pending="modelRouteWhitelistPending"',
  )
  expect(page).toContain(
    ':stream-delta-batching-pending="streamDeltaBatchingPending"',
  )
  expect(requests).toContain('v-if="modelRouteWhitelistPending"')
  expect(requests).toContain(':disabled="modelRouteWhitelistPending"')
  expect(requests).toContain('v-if="streamDeltaBatchingPending"')
  expect(requests).toContain(':disabled="streamDeltaBatchingPending"')
  const streamControls = requests.slice(
    requests.indexOf('<fieldset'),
    requests.indexOf('</fieldset>'),
  )
  expect(streamControls).toContain(':aria-busy="streamDeltaBatchingPending"')
  expect(streamControls).toContain('<StreamDeltaBatchingFields')
})

test('shared retention pending disables both forms and spins only the origin', async () => {
  const [requests, page] = await Promise.all([
    source('../src/components/settings/SettingsRequestsTab.vue'),
    source('../src/pages/SettingsPage.vue'),
  ])

  expect(page).toContain(
    ':pending-retention-form="settingsStore.pendingRetentionForm"',
  )
  expect(requests).toContain('retentionPending = computed(')
  expect(requests).toContain(
    "props.pendingRetentionForm === 'requestContentLogging'",
  )
  expect(requests).toContain("props.pendingRetentionForm === 'usageRetention'")
  expect(requests.match(/:disabled="retentionPending"/g)?.length).toBe(3)
  expect(requests.match(/:aria-busy="retentionPending"/g)?.length).toBe(3)
  expect(requests).toContain('v-if="requestContentLoggingPending"')
  expect(requests).toContain('v-if="usageRetentionPending"')
})
