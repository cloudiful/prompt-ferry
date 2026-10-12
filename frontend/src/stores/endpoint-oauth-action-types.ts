import type { AsyncActionOutcome } from '@/composables/useAsyncAction'

export type RunEndpointOAuthAction = <T>(
  endpointId: string,
  action: () => Promise<T>,
  label: string,
) => Promise<AsyncActionOutcome<T>>
