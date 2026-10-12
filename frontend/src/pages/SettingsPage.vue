<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import PageIntro from '../components/PageIntro.vue'
import { setLocale, useLocale } from '../composables/useLocale'
import { useNotifier } from '../composables/useNotifier'
import { useSettingsPageActions } from '../composables/useSettingsPageActions'
import SettingsGeneralTab from '../components/settings/SettingsGeneralTab.vue'
import SettingsCacheAlertSection from '../components/settings/SettingsCacheAlertSection.vue'
import SettingsConfigArchiveTab from '../components/settings/SettingsConfigArchiveTab.vue'
import SettingsNetworkTab from '../components/settings/SettingsNetworkTab.vue'
import SettingsRequestsTab from '../components/settings/SettingsRequestsTab.vue'
import SettingsReviewTab from '../components/settings/SettingsReviewTab.vue'
import SettingsStorageTab from '../components/settings/SettingsStorageTab.vue'
import { resolveSettingsTab } from '../models/settings'
import { useSessionStore } from '../stores/session'
import { useSettingsStore } from '../stores/settings'
import { settingFormKey } from '../stores/settings-actions'
import { setThemeMode, useThemeMode } from '@/theme/appTheme'

const session = useSessionStore()
const settingsStore = useSettingsStore()
const route = useRoute()
const { locale, t } = useLocale()
const themeMode = useThemeMode()
const { notifyApiError } = useNotifier()

const activeSection = computed(() =>
  resolveSettingsTab(route.path.split('/')[2] ?? null, session.isAdmin),
)

const { adoptLoadedSettings, dispose, saveRelayIpWhitelist, saveLlmReview } =
  useSettingsPageActions({ activeSection, store: settingsStore, t })

onBeforeUnmount(dispose)

const relayIpWhitelistPending = computed(
  () => !!settingsStore.settingsPendingActions.relayIpWhitelist,
)
const llmReviewPending = computed(
  () => !!settingsStore.settingsPendingActions.llmReview,
)
const modelRouteWhitelistPending = computed(() =>
  settingsStore.isPending(settingFormKey('modelRouteWhitelist')),
)
const streamDeltaBatchingPending = computed(() =>
  settingsStore.isPending(settingFormKey('streamDeltaBatching')),
)

async function refresh(): Promise<void> {
  try {
    await settingsStore.refresh()
    adoptLoadedSettings()
    await session.refreshBridgeStatus()
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(async () => {
  if (session.isAdmin) {
    await refresh()
  } else {
    await session.refreshBridgeStatus()
  }
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro>
      <template #actions>
        <UButton
          v-if="session.isAdmin"
          size="sm"
          color="neutral"
          variant="outline"
          icon="i-lucide-refresh-cw"
          :loading="settingsStore.loading"
          :aria-label="t('refresh')"
          @click="refresh"
        >
          <span class="hidden sm:inline">{{ t('refresh') }}</span>
        </UButton>
      </template>
    </PageIntro>

    <SettingsGeneralTab
      v-if="activeSection === 'general'"
      v-model:locale="locale"
      v-model:theme-mode="themeMode"
      :t="t"
      @set-locale="setLocale"
      @set-theme-mode="setThemeMode"
    />

    <SettingsRequestsTab
      v-else-if="session.isAdmin && activeSection === 'requests'"
      v-model:request-content-logging="settingsStore.requestContentLogging"
      v-model:usage-retention="settingsStore.usageRetention"
      v-model:stream-delta-batching="settingsStore.streamDeltaBatching"
      v-model:model-route-whitelist="settingsStore.modelRouteWhitelist"
      :pending-retention-form="settingsStore.pendingRetentionForm"
      :model-route-whitelist-pending="modelRouteWhitelistPending"
      :stream-delta-batching-pending="streamDeltaBatchingPending"
      :t="t"
    />

    <SettingsNetworkTab
      v-else-if="session.isAdmin && activeSection === 'network'"
      v-model:relay-ip-whitelist="settingsStore.relayIpWhitelist"
      :busy="settingsStore.loading"
      :pending="relayIpWhitelistPending"
      :t="t"
      @save-relay-ip-whitelist="saveRelayIpWhitelist"
    />

    <SettingsReviewTab
      v-else-if="session.isAdmin && activeSection === 'review'"
      v-model:llm-review="settingsStore.llmReview"
      v-model:llm-review-webhook-headers-text="
        settingsStore.llmReviewWebhookHeadersText
      "
      :busy="settingsStore.loading"
      :pending="llmReviewPending"
      :t="t"
      @save-llm-review="saveLlmReview"
    />

    <SettingsStorageTab
      v-else-if="session.isAdmin && activeSection === 'storage'"
      :t="t"
    />

    <SettingsCacheAlertSection
      v-else-if="session.isAdmin && activeSection === 'cache-alert'"
      :t="t"
    />

    <SettingsConfigArchiveTab
      v-else-if="session.isAdmin && activeSection === 'config-archive'"
      :t="t"
    />
  </div>
</template>
