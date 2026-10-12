import type { Ref } from 'vue'

import { createDefaultRequestRecordClearForm } from '@/admin-mappers'
import { useNotifier } from '@/composables/useNotifier'
import { useLocale } from '@/composables/useLocale'
import type { RequestRecordClearForm } from '@/models'
import type { useRequestRecordsStore } from '@/stores/usage'

export type RequestRecordsStore = ReturnType<typeof useRequestRecordsStore>
type Translate = ReturnType<typeof useLocale>['t']

export function useRequestRecordsPageActions(options: {
  store: RequestRecordsStore
  clearDialogVisible: Ref<boolean>
  clearForm: Ref<RequestRecordClearForm>
  notifyApiError: ReturnType<typeof useNotifier>['notifyApiError']
  notifySuccess: ReturnType<typeof useNotifier>['notifySuccess']
  t: Translate
}) {
  const {
    clearDialogVisible,
    clearForm,
    notifyApiError,
    notifySuccess,
    store,
    t,
  } = options

  async function submitClearHistory(): Promise<void> {
    const submittedForm = clearForm.value
    try {
      const outcome = await store.clearHistory(submittedForm)
      if (!outcome.executed) return
      notifySuccess(`${t('clearHistorySubmit')}: ${outcome.value.deleted}`)
      if (clearForm.value === submittedForm) {
        clearDialogVisible.value = false
        clearForm.value = createDefaultRequestRecordClearForm()
      }
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function setAiPerspective(
    nextPerspective: Parameters<RequestRecordsStore['setAiPerspective']>[0],
  ): Promise<void> {
    try {
      await store.setAiPerspective(nextPerspective)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return { setAiPerspective, submitClearHistory }
}
