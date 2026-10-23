import { computed, type Ref } from 'vue'
import type { EndpointForm } from '@/models'
import {
  scheduleRowError,
  scheduleWindowsForDisplay,
} from '@/models/endpoints/schedule-windows'

// Issue #589 P2c: the endpoint dialog's proxy/schedule validation and settings
// predicates live here so the dialog stays a thin composition shell. The
// proxy (#368 Phase C) rules are unchanged; the schedule rules come from the
// shared helper (#457 / #392 Phase L, #1045).
export function useEndpointDialogValidation(
  form: Ref<EndpointForm | undefined>,
  t: TranslateFn,
) {
  // INLINE-proxy-ui-a1 BUG fix: legacy forms may omit Phase C fields; guard
  // so the dialog renders instead of throwing on undefined access.
  const hasProxy = computed(() => {
    const typed = (form.value?.proxy_url ?? '').trim() !== ''
    const saved = form.value?.has_saved_proxy_url ?? false
    return typed || saved
  })

  // Issue #368 Phase C: empty means direct/keep (valid); `scheme://` with an
  // empty address is an unfinished inline edit and must block the outer save.
  const isProxyValid = computed(() => {
    const trimmed = (form.value?.proxy_url ?? '').trim()
    if (!trimmed) return true
    const match = trimmed.match(/^(http|https|socks5h|socks5):\/\/(.*)$/i)
    if (match) return (match[2] ?? '').trim() !== ''
    return true
  })

  function endpointRowError(window: { start: string; end: string }): string {
    return scheduleRowError(window, t)
  }

  const isScheduleValid = computed(() =>
    (form.value?.active_windows ?? []).every(
      (window) => endpointRowError(window) === '',
    ),
  )

  const canSaveOuter = computed(
    () => isProxyValid.value && isScheduleValid.value,
  )

  // Issue #392 Phase L: endpoint default schedule mirrors the target dialog.
  // Non-empty means restricted; empty means all-day.
  function endpointWindows(): Array<{ start: string; end: string }> {
    return Array.isArray(form.value?.active_windows)
      ? (form.value?.active_windows ?? [])
      : []
  }

  function sortedEndpointWindows(): Array<{ start: string; end: string }> {
    return scheduleWindowsForDisplay(endpointWindows())
      .map((window) => ({ start: window.start, end: window.end }))
      .sort((a, b) =>
        a.start === b.start
          ? a.end.localeCompare(b.end)
          : a.start.localeCompare(b.start),
      )
  }

  function hasEndpointSchedule(): boolean {
    return sortedEndpointWindows().length > 0
  }

  // Gear highlight when anything is non-default.
  function hasEndpointSettings(): boolean {
    return hasProxy.value || hasEndpointSchedule()
  }

  return {
    canSaveOuter,
    hasEndpointSettings,
  }
}
