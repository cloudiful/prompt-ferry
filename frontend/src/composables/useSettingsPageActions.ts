/**
 * Page-level save handling for the settings tabs.
 *
 * Four of the settings sections are autosaved and two are saved by a button. The
 * gate itself belongs to the store — it is per resource, so one resource saving
 * never blocks an unrelated one — but what a *page* owes the operator is
 * narrower: an autosave that the gate declined must not record its form as
 * saved, and a button save that was declined must not toast a success nobody
 * earned.
 *
 * The retention forms are why that matters here. They share one key, so an edit
 * to one routinely finds the other's save in flight. The held-off form stays
 * dirty and is retried once the key is released; the operator's edit is never
 * silently dropped, and never reported as saved either.
 *
 * Both live here so the page is left with wiring and the autosave schedule. The
 * store is injected, as in the record-domain composables, so the completion
 * contract can be driven without a page.
 */

import { ref, watch, type ComputedRef } from 'vue'

import { useNotifier } from './useNotifier'
import type { AsyncActionOutcome } from './useAsyncAction'
import type { SettingsTab } from '@/models/settings'
import { settingFormKey } from '@/stores/settings-actions'
import type { SettingForm } from '@/stores/settings-actions'
import type { useSettingsStore } from '@/stores/settings'

type SettingsStore = ReturnType<typeof useSettingsStore>

type AutosaveOptions = {
  delayMs: number
  getSnapshot: () => string
  save: () => Promise<AsyncActionOutcome<unknown>>
}

/** The tab whose forms autosave; elsewhere the saves are explicit buttons. */
const AUTOSAVE_SECTION = 'requests'

export function useSettingsPageActions(options: {
  activeSection: ComputedRef<SettingsTab>
  store: SettingsStore
  t: (key: string) => string
}) {
  const { activeSection, store: settingsStore, t } = options
  const { notifyApiError, notifySuccess } = useNotifier()

  const autosaveReady = ref(false)
  const snapshots = ref<Record<string, string>>({})

  const serialize = (value: unknown): string => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) {
      return JSON.stringify(value) ?? ''
    }
    return JSON.stringify(
      Object.fromEntries(
        Object.entries(value).sort(([left], [right]) =>
          left.localeCompare(right),
        ),
      ),
    )
  }

  function savedSnapshot(section: string): string {
    return snapshots.value[section] ?? ''
  }

  function setSavedSnapshot(section: string, value: string): void {
    snapshots.value = { ...snapshots.value, [section]: value }
  }

  /** Adopt whatever the server last confirmed, so mounting is not a change. */
  function syncSnapshots(): void {
    snapshots.value = {
      requestContentLogging: serialize(settingsStore.requestContentLogging),
      usageRetention: serialize(settingsStore.usageRetention),
      streamDeltaBatching: serialize(settingsStore.streamDeltaBatching),
      modelRouteWhitelist: serialize(settingsStore.modelRouteWhitelist),
    }
  }

  function canAutosave(): boolean {
    return (
      autosaveReady.value &&
      !settingsStore.loading &&
      activeSection.value === AUTOSAVE_SECTION
    )
  }

  function createAutosaveController(
    section: SettingForm,
    {
      delayMs,
      getSnapshot,
      save,
    }: Pick<AutosaveOptions, 'delayMs' | 'getSnapshot' | 'save'>,
  ): { schedule: () => void; dispose: () => void } {
    let timer: ReturnType<typeof setTimeout> | null = null
    let saving = false
    let inFlightSnapshot = ''
    let queued = false
    let waitingForKey = false
    const key = settingFormKey(section)

    async function flush(): Promise<void> {
      timer = null
      if (!canAutosave()) return
      const submittedSnapshot = getSnapshot()
      if (saving) {
        if (submittedSnapshot !== inFlightSnapshot) queued = true
        return
      }
      if (submittedSnapshot === savedSnapshot(section)) return

      saving = true
      inFlightSnapshot = submittedSnapshot
      try {
        const outcome = await save()
        if (outcome.executed) {
          const confirmedSnapshot =
            outcome.value === undefined
              ? submittedSnapshot
              : serialize(outcome.value)
          setSavedSnapshot(section, confirmedSnapshot)
        } else {
          // Another form may own this shared resource key. Keep the form dirty,
          // and wait for that owner to release it rather than spinning retries.
          queued = true
          waitingForKey = true
        }
      } catch (cause) {
        notifyApiError(cause)
      } finally {
        saving = false
        inFlightSnapshot = ''
        if (queued && !waitingForKey) {
          queued = false
          void flush()
        }
      }
    }

    const stopPendingWatch = watch(
      () => settingsStore.isPending(key),
      (pending) => {
        if (pending || !waitingForKey) return
        waitingForKey = false
        if (saving) return
        if (queued) {
          queued = false
          void flush()
        }
      },
    )

    function schedule(): void {
      if (!canAutosave()) return
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        void flush()
      }, delayMs)
    }

    function dispose(): void {
      stopPendingWatch()
      if (timer) {
        clearTimeout(timer)
        timer = null
      }
    }

    return { schedule, dispose }
  }

  const controllers = [
    createAutosaveController('requestContentLogging', {
      delayMs: 250,
      getSnapshot: () => serialize(settingsStore.requestContentLogging),
      save: () => settingsStore.saveRequestContentLogging(),
    }),
    createAutosaveController('usageRetention', {
      delayMs: 250,
      getSnapshot: () => serialize(settingsStore.usageRetention),
      save: () => settingsStore.saveUsageRetention(),
    }),
    createAutosaveController('streamDeltaBatching', {
      delayMs: 300,
      getSnapshot: () => serialize(settingsStore.streamDeltaBatching),
      save: () => settingsStore.saveStreamDeltaBatching(),
    }),
    createAutosaveController('modelRouteWhitelist', {
      delayMs: 150,
      getSnapshot: () => serialize(settingsStore.modelRouteWhitelist),
      save: () => settingsStore.saveModelRouteWhitelist(),
    }),
  ]

  const sources: Array<[() => unknown, () => void]> = [
    [() => settingsStore.requestContentLogging, controllers[0].schedule],
    [() => settingsStore.usageRetention, controllers[1].schedule],
    [() => settingsStore.streamDeltaBatching, controllers[2].schedule],
    [() => settingsStore.modelRouteWhitelist, controllers[3].schedule],
  ]

  function watchAutosaveSources(): void {
    for (const [source, schedule] of sources) {
      watch(source, schedule, { deep: true })
    }
  }

  // Watching is unconditional; `schedule` no-ops until the first load has been
  // adopted, so mounting the page registers these exactly once and nothing
  // fires early.
  watchAutosaveSources()

  /**
   * Adopt a freshly loaded settings state as already-saved.
   *
   * Until this runs the forms must not autosave, or simply loading a tab would
   * be read as an edit worth writing back. A refresh repeats it: the newly
   * loaded state is the server's truth, not an operator edit.
   */
  function adoptLoadedSettings(): void {
    syncSnapshots()
    autosaveReady.value = true
  }

  function dispose(): void {
    for (const controller of controllers) {
      controller.dispose()
    }
  }

  /** Report only a save that actually reached the server. */
  async function saveAndReport(
    save: () => Promise<AsyncActionOutcome<void>>,
    message: string,
  ): Promise<void> {
    try {
      const outcome = await save()
      if (outcome.executed) {
        notifySuccess(message)
      }
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return {
    adoptLoadedSettings,
    dispose,
    saveRelayIpWhitelist: () =>
      saveAndReport(() => settingsStore.saveRelayIpWhitelist(), t('saved')),
    saveLlmReview: () =>
      saveAndReport(() => settingsStore.saveLlmReview(), t('llmReviewSaved')),
  }
}
