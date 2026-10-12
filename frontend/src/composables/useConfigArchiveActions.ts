import { useNotifier } from './useNotifier'
import type { AsyncActionOutcome } from './useAsyncAction'
import type { useConfigArchiveStore } from '@/stores/settings/config-archive'

type ConfigArchiveStore = ReturnType<typeof useConfigArchiveStore>

export function useConfigArchiveActions(options: {
  store: ConfigArchiveStore
  t: (key: string) => string
  onApplied?: () => void
}) {
  const { notifyApiError, notifySuccess } = useNotifier()

  async function report<T>(
    action: () => Promise<AsyncActionOutcome<T>>,
    successMessage?: string,
    onSuccess?: () => void,
  ): Promise<boolean> {
    try {
      const outcome = await action()
      if (!outcome.executed) return false
      if (successMessage) notifySuccess(options.t(successMessage))
      onSuccess?.()
      return true
    } catch (cause) {
      notifyApiError(cause)
      return false
    }
  }

  return {
    exportArchive: () =>
      report(() => options.store.exportArchive(), 'configArchiveExportDone'),
    previewImport: () => report(() => options.store.previewImport()),
    confirmApply: () =>
      report(
        () => options.store.applyImport(),
        'configArchiveImportApplied',
        options.onApplied,
      ),
  }
}
