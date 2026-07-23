<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useLocale } from '../composables/useLocale'
import SettingsGeneralTab from '../components/settings/SettingsGeneralTab.vue'
import SettingsNetworkTab from '../components/settings/SettingsNetworkTab.vue'
import SettingsRequestsTab from '../components/settings/SettingsRequestsTab.vue'
import SettingsReviewTab from '../components/settings/SettingsReviewTab.vue'
import { setLocale } from '../composables/useLocale'
import { useNotifier } from '../composables/useNotifier'
import { resolveSettingsTab } from '../models/settings'
import { useSessionStore } from '../stores/session'
import { useSettingsStore } from '../stores/settings'
import { setThemeMode, useThemeMode } from '@/theme/appTheme'

const session = useSessionStore()
const settingsStore = useSettingsStore()
const route = useRoute()
const { locale, t } = useLocale()
const themeMode = useThemeMode()
const { notifyApiError, notifySuccess } = useNotifier()

const activeSection = computed(() =>
  resolveSettingsTab(route.path.split('/')[2] ?? null, session.isAdmin),
)
const requestAutosaveReady = { value: false }
let requestContentLoggingSnapshot = ''
let streamDeltaBatchingSnapshot = ''
let modelRouteWhitelistSnapshot = ''

function serialize(value: unknown): string {
  return JSON.stringify(value)
}

function syncRequestSnapshots(): void {
  requestContentLoggingSnapshot = serialize(settingsStore.requestContentLogging)
  streamDeltaBatchingSnapshot = serialize(settingsStore.streamDeltaBatching)
  modelRouteWhitelistSnapshot = serialize(settingsStore.modelRouteWhitelist)
}

function createAutosaveController(options: {
  delayMs: number
  getSnapshot: () => string
  getSavedSnapshot: () => string
  setSavedSnapshot: (value: string) => void
  save: () => Promise<void>
}): { schedule: () => void; dispose: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null
  let saving = false
  let queued = false

  async function flush(): Promise<void> {
    timer = null
    if (
      !requestAutosaveReady.value ||
      settingsStore.loading ||
      activeSection.value !== 'requests'
    ) {
      return
    }

    const snapshot = options.getSnapshot()
    if (snapshot === options.getSavedSnapshot()) {
      return
    }

    if (saving) {
      queued = true
      return
    }

    saving = true
    try {
      await options.save()
      options.setSavedSnapshot(options.getSnapshot())
    } catch (cause) {
      notifyApiError(cause)
    } finally {
      saving = false
      if (queued) {
        queued = false
        void flush()
      }
    }
  }

  function schedule(): void {
    if (
      !requestAutosaveReady.value ||
      settingsStore.loading ||
      activeSection.value !== 'requests'
    ) {
      return
    }

    if (timer) {
      clearTimeout(timer)
    }
    timer = setTimeout(() => {
      void flush()
    }, options.delayMs)
  }

  function dispose(): void {
    if (timer) {
      clearTimeout(timer)
      timer = null
    }
  }

  return { schedule, dispose }
}

const requestContentLoggingAutosave = createAutosaveController({
  delayMs: 250,
  getSnapshot: () => serialize(settingsStore.requestContentLogging),
  getSavedSnapshot: () => requestContentLoggingSnapshot,
  setSavedSnapshot: (value) => {
    requestContentLoggingSnapshot = value
  },
  save: () => settingsStore.saveRequestContentLogging(),
})

const streamDeltaBatchingAutosave = createAutosaveController({
  delayMs: 300,
  getSnapshot: () => serialize(settingsStore.streamDeltaBatching),
  getSavedSnapshot: () => streamDeltaBatchingSnapshot,
  setSavedSnapshot: (value) => {
    streamDeltaBatchingSnapshot = value
  },
  save: () => settingsStore.saveStreamDeltaBatching(),
})

const modelRouteWhitelistAutosave = createAutosaveController({
  delayMs: 150,
  getSnapshot: () => serialize(settingsStore.modelRouteWhitelist),
  getSavedSnapshot: () => modelRouteWhitelistSnapshot,
  setSavedSnapshot: (value) => {
    modelRouteWhitelistSnapshot = value
  },
  save: () => settingsStore.saveModelRouteWhitelist(),
})

async function refresh(): Promise<void> {
  try {
    await settingsStore.refresh()
    syncRequestSnapshots()
    requestAutosaveReady.value = true
    await session.refreshBridgeStatus()
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function saveRelayIpWhitelist(): Promise<void> {
  try {
    await settingsStore.saveRelayIpWhitelist()
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function saveLlmReview(): Promise<void> {
  try {
    await settingsStore.saveLlmReview()
    notifySuccess(t('llmReviewSaved'))
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

watch(
  () => settingsStore.requestContentLogging,
  () => {
    requestContentLoggingAutosave.schedule()
  },
  { deep: true },
)

watch(
  () => settingsStore.streamDeltaBatching,
  () => {
    streamDeltaBatchingAutosave.schedule()
  },
  { deep: true },
)

watch(
  () => settingsStore.modelRouteWhitelist,
  () => {
    modelRouteWhitelistAutosave.schedule()
  },
  { deep: true },
)

onBeforeUnmount(() => {
  requestContentLoggingAutosave.dispose()
  streamDeltaBatchingAutosave.dispose()
  modelRouteWhitelistAutosave.dispose()
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
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
      v-model:stream-delta-batching="settingsStore.streamDeltaBatching"
      v-model:model-route-whitelist="settingsStore.modelRouteWhitelist"
      :t="t"
    />

    <SettingsNetworkTab
      v-else-if="session.isAdmin && activeSection === 'network'"
      v-model:relay-ip-whitelist="settingsStore.relayIpWhitelist"
      :busy="settingsStore.loading"
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
      :t="t"
      @save-llm-review="saveLlmReview"
    />
  </div>
</template>
