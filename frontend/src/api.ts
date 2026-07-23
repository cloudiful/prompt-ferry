import { client } from './generated/admin-api/client.gen'

client.setConfig({
  baseUrl: '',
  credentials: 'include',
  responseStyle: 'data',
  throwOnError: true,
})

export { client }

export function withData<T extends object | undefined>(
  options?: T,
): T & { responseStyle: 'data' } {
  return {
    ...(options ?? {}),
    responseStyle: 'data',
  } as T & { responseStyle: 'data' }
}

export function expectData<T>(result: T | { data: T }): T {
  if (result && typeof result === 'object' && 'data' in result) {
    return result.data
  }
  return result
}

function extractApiErrorMessage(error: unknown): string | null {
  if (!error) return null
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  if (typeof error !== 'object') return null

  if (
    'message' in error &&
    typeof (error as { message?: unknown }).message === 'string'
  ) {
    return (error as { message: string }).message
  }

  if ('error' in error) {
    const envelope = (error as { error?: unknown }).error
    if (envelope && typeof envelope === 'object') {
      const code =
        'code' in envelope &&
        typeof (envelope as { code?: unknown }).code === 'string'
          ? (envelope as { code: string }).code
          : null
      const message =
        'message' in envelope &&
        typeof (envelope as { message?: unknown }).message === 'string'
          ? (envelope as { message: string }).message
          : null
      if (code && message) return `${code}: ${message}`
      if (message) return message
      if (code) return code
    }
  }

  return null
}

export function formatApiError(error: unknown): string {
  return extractApiErrorMessage(error) ?? 'Request failed'
}
