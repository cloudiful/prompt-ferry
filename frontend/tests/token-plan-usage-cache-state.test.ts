import {
  afterAll,
  afterEach,
  beforeEach,
  expect,
  jest,
  mock,
  test,
} from 'bun:test'
import type { TokenPlanUsageResponse } from '../src/generated/admin-api'
import { minimaxQuotaKey, usage } from './helpers/token-plan-quota-fixtures'

const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
if (typeof window === 'undefined') {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {},
  })
}

const fetchTokenPlanUsage = mock(
  async (_endpointId: string): Promise<TokenPlanUsageResponse> => ({
    keys: [],
    provider: 'minimax',
    provider_region: null,
  }),
)

mock.module('../src/stores/endpoints-api', () => ({ fetchTokenPlanUsage }))

const {
  __resetTokenPlanCacheForTests,
  prefetchTokenPlanUsage,
  requestTokenPlanUsage,
  TOKEN_PLAN_CACHE_TTL_MS,
} = await import('../src/composables/useTokenPlanUsageCache')
const { useTokenPlanBadges } =
  await import('../src/composables/useTokenPlanBadges')

beforeEach(() => {
  jest.useFakeTimers()
  jest.setSystemTime(new Date('2026-10-11T00:00:00.000Z'))
  __resetTokenPlanCacheForTests()
  fetchTokenPlanUsage.mockReset()
})

afterEach(() => {
  __resetTokenPlanCacheForTests()
  jest.useRealTimers()
})

afterAll(() => {
  if (previousWindow) {
    Object.defineProperty(globalThis, 'window', previousWindow)
  } else {
    Reflect.deleteProperty(globalThis, 'window')
  }
})

test('mounted desktop and mobile badge computeds become stale at cache expiry', async () => {
  const snapshot = usage('minimax', [
    minimaxQuotaKey('key', 'available', 50, 35),
  ])
  fetchTokenPlanUsage.mockResolvedValueOnce(snapshot)
  await prefetchTokenPlanUsage('ep-reactive-expiry')

  const desktopBadges = useTokenPlanBadges('ep-reactive-expiry')
  const mobileBadges = useTokenPlanBadges('ep-reactive-expiry')
  expect(desktopBadges.value.status).toBe('ready')
  expect(desktopBadges.value.quotaState).toBe('available')
  expect(desktopBadges.value.short).toBe(50)
  expect(mobileBadges.value.status).toBe('ready')

  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS)

  expect(desktopBadges.value.status).toBe('stale')
  expect(desktopBadges.value.quotaState).toBe('unknown')
  expect(desktopBadges.value.short).toBeNull()
  expect(desktopBadges.value.long).toBeNull()
  expect(desktopBadges.value.usage).toEqual(snapshot)
  expect(mobileBadges.value.status).toBe('stale')
  expect(mobileBadges.value.quotaState).toBe('unknown')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(1)
})

test('a successful refresh replaces the old expiry deadline', async () => {
  const first = usage('minimax', [minimaxQuotaKey('key', 'available', 50, 35)])
  const second = usage('minimax', [minimaxQuotaKey('key', 'available', 75, 65)])
  fetchTokenPlanUsage.mockResolvedValueOnce(first)
  await prefetchTokenPlanUsage('ep-replaced-expiry')
  const badges = useTokenPlanBadges('ep-replaced-expiry')

  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS / 2)
  fetchTokenPlanUsage.mockResolvedValueOnce(second)
  await requestTokenPlanUsage('ep-replaced-expiry', { refresh: true })
  expect(badges.value.usage).toEqual(second)

  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS / 2)
  expect(badges.value.status).toBe('ready')
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS / 2)
  expect(badges.value.status).toBe('stale')
  expect(fetchTokenPlanUsage).toHaveBeenCalledTimes(2)
})

test('reset cancels the cache-owned expiry timer', async () => {
  fetchTokenPlanUsage.mockResolvedValueOnce(
    usage('minimax', [minimaxQuotaKey('key', 'available', 50, 35)]),
  )
  await prefetchTokenPlanUsage('ep-reset-expiry')
  expect(jest.getTimerCount()).toBe(1)

  __resetTokenPlanCacheForTests()

  expect(jest.getTimerCount()).toBe(0)
  jest.advanceTimersByTime(TOKEN_PLAN_CACHE_TTL_MS)
  expect(jest.getTimerCount()).toBe(0)
})
