import { ref } from 'vue'
import type { RequestRecordCategory } from '../generated/admin-api'
import type { RequestOverviewMode } from '../request-overview'

const overviewModeStorageKey = 'request-records-overview-mode'

export function useRequestOverviewMode(
  getCategory: () => RequestRecordCategory,
) {
  const activeMode = ref<RequestOverviewMode>('overview')

  function syncMode(category: RequestRecordCategory): void {
    activeMode.value = loadMode(category)
  }

  function setActiveMode(mode: RequestOverviewMode): void {
    activeMode.value = mode
    saveMode(getCategory(), mode)
  }

  return {
    activeMode,
    setActiveMode,
    syncMode,
  }
}

function loadMode(category: RequestRecordCategory): RequestOverviewMode {
  try {
    const raw = window.localStorage.getItem(overviewModeStorageKey)
    const parsed = raw
      ? (JSON.parse(raw) as Partial<
          Record<RequestRecordCategory, RequestOverviewMode>
        >)
      : {}
    return parsed[category] ?? 'overview'
  } catch {
    return 'overview'
  }
}

function saveMode(
  category: RequestRecordCategory,
  mode: RequestOverviewMode,
): void {
  try {
    const raw = window.localStorage.getItem(overviewModeStorageKey)
    const parsed = raw
      ? (JSON.parse(raw) as Partial<
          Record<RequestRecordCategory, RequestOverviewMode>
        >)
      : {}
    parsed[category] = mode
    window.localStorage.setItem(overviewModeStorageKey, JSON.stringify(parsed))
  } catch {
    // ignore
  }
}
