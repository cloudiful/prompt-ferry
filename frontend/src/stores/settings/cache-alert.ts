import { ref } from 'vue'
import {
  getCacheAlertSetting,
  setCacheAlertSetting,
} from '../../generated/admin-api'
import { cacheAlertFormToRequest, cacheAlertToForm } from '../../admin-mappers'
import type { CacheAlertForm } from '../../admin-mappers'
import { expectData, withData } from '../../api'

export function createCacheAlertStore() {
  const cacheAlert = ref<CacheAlertForm | null>(null)
  const cacheAlertError = ref<string | null>(null)

  async function refreshCacheAlert(): Promise<void> {
    cacheAlert.value = cacheAlertToForm(
      expectData(await getCacheAlertSetting<true>(withData())),
    )
    cacheAlertError.value = null
  }

  // `refresh()` must keep the other tabs usable when the cache-alert
  // capability is unavailable (SQLite), mirroring the raw object store path.
  async function loadCacheAlert(): Promise<void> {
    try {
      await refreshCacheAlert()
    } catch (cause) {
      cacheAlert.value = null
      cacheAlertError.value =
        cause instanceof Error ? cause.message : String(cause ?? '')
    }
  }

  async function saveCacheAlert(): Promise<void> {
    const form = cacheAlert.value
    if (!form) return
    cacheAlert.value = cacheAlertToForm(
      expectData(
        await setCacheAlertSetting<true>(
          withData({ body: cacheAlertFormToRequest(form) }),
        ),
      ),
    )
    cacheAlertError.value = null
  }

  return {
    cacheAlert,
    cacheAlertError,
    loadCacheAlert,
    refreshCacheAlert,
    saveCacheAlert,
  }
}
