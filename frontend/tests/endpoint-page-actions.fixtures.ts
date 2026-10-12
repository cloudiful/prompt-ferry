import { mock } from 'bun:test'
import { reactive } from 'vue'

import type {
  EndpointRequest,
  ProviderEndpoint,
} from '../src/generated/admin-api'
import { useEndpointPageActions } from '../src/composables/useEndpointPageActions'

/**
 * Shared setup for the page action tests.
 *
 * The page actions take their store, their notifier, and their confirmation as
 * dependencies, so they can be driven against a recorded fake instead of a real
 * store, toast stack, or `window.confirm`. Nothing here renders or requests.
 */

export const t = (key: string) => key

export const endpoint = (id: string) =>
  ({
    enabled: true,
    endpoint_id: id,
    name: `upstream ${id}`,
  }) as ProviderEndpoint

export const route = (id: string) =>
  ({ enabled: true, model_pattern: `model-${id}`, rule_id: id }) as never

const endpoints = [endpoint('endpoint-a'), endpoint('endpoint-b')]
const routes = [route('rule-a'), route('rule-b')]

/** A gate the tests open by hand, so "in flight" is really in flight. */
export function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

export type PageOptions = { confirm?: boolean; executed?: boolean }

/**
 * A page over a store-shaped object, with the notifier and the confirmation
 * recorded instead of rendered. Only the surface the composable actually
 * touches is present.
 */
export function buildPage(options: PageOptions = {}) {
  const pending = reactive({
    endpoint: {} as Record<string, string>,
    modelRoute: {} as Record<string, string>,
  })
  const notices = { errors: [] as unknown[], successes: [] as string[] }
  const confirmations: string[] = []
  const answered = async () => ({ executed: options.executed ?? true })
  const answeredWith = async (value: unknown) => ({
    executed: options.executed ?? true,
    value,
  })

  const store = {
    get endpointPendingActions() {
      return pending.endpoint
    },
    get modelRoutePendingActions() {
      return pending.modelRoute
    },
    findEndpointById: (id: string) =>
      endpoints.find((item) => item.endpoint_id === id) ?? null,
    findModelRouteById: (id: string) =>
      routes.find((item) => item.rule_id === id) ?? null,
    removeEndpoint: mock(async (_id: string) => answered()),
    removeModelRoute: mock(async (_id: string) => answered()),
    runEndpointTest: mock(async (_id: string) => answered()),
    runModelRouteTest: mock(async (_id: string) => answered()),
    saveEndpoint: mock(async (_id: string | null, _body: EndpointRequest) =>
      answeredWith(endpoint('endpoint-a')),
    ),
    saveModelRoute: mock(async (_id: string | null, _body: unknown) =>
      answeredWith(route('rule-a')),
    ),
    toggleEndpointEnabled: mock(async (_id: string, _enabled: boolean) =>
      answeredWith(endpoint('endpoint-a')),
    ),
    toggleModelRouteEnabled: mock(async (_id: string, _enabled: boolean) =>
      answeredWith(route('rule-a')),
    ),
  }

  const page = useEndpointPageActions({
    confirm: (message) => {
      confirmations.push(message)
      return options.confirm ?? true
    },
    notifyApiError: (cause) => notices.errors.push(cause),
    notifySuccess: (message) => notices.successes.push(message),
    store: store as never,
    t: t as never,
  })

  return { confirmations, notices, page, pending, store }
}

export type Page = ReturnType<typeof buildPage>

/** A fresh page per test, with empty notice and confirmation records. */
export const freshPage = (options?: PageOptions): Page => buildPage(options)
