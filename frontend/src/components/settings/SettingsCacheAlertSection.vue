<script setup lang="ts">
import { useNotifier } from '@/composables/useNotifier'
import { useSettingsStore } from '@/stores/settings'
import SettingsCacheAlertTab from './SettingsCacheAlertTab.vue'

const props = defineProps<{
  t: TranslateFn
}>()

const settingsStore = useSettingsStore()
const { notifyApiError, notifySuccess } = useNotifier()

async function saveCacheAlert(): Promise<void> {
  try {
    await settingsStore.saveCacheAlert()
    notifySuccess(props.t('cacheAlertSaved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function retryCacheAlert(): Promise<void> {
  try {
    await settingsStore.refreshCacheAlert()
  } catch (cause) {
    notifyApiError(cause)
  }
}
</script>

<template>
  <SettingsCacheAlertTab
    v-model:cache-alert="settingsStore.cacheAlert"
    :busy="settingsStore.loading"
    :error="settingsStore.cacheAlertError"
    :t="t"
    @save-cache-alert="saveCacheAlert"
    @retry-cache-alert="retryCacheAlert"
  />
</template>
