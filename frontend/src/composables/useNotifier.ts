import { formatApiError } from '../api'
import { useToast } from '@nuxt/ui/composables'

export function useNotifier() {
  const toast = useToast()

  function add(
    color: 'error' | 'info' | 'success',
    description: string,
    title?: string,
  ): void {
    toast.add({ color, description, title })
  }

  return {
    notifyApiError(cause: unknown, title?: string): void {
      add('error', formatApiError(cause), title)
    },
    notifyError(message: string, title?: string): void {
      add('error', message, title)
    },
    notifyInfo(message: string, title?: string): void {
      add('info', message, title)
    },
    notifySuccess(message: string, title?: string): void {
      add('success', message, title)
    },
  }
}
