import { computed } from 'vue'
import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  RequestRecordCategory,
  RequestRecordOverviewRange,
  RequestRecordOverviewResponse,
} from '@/generated/admin-api'
import type { RequestOverviewPerspective } from '@/request-overview'

export const USAGE_PERSPECTIVE_KEY = 'usage:perspective'

export type UsageOverviewRequest = {
  requestCategory: RequestRecordCategory
  perspective: RequestOverviewPerspective
  range: RequestRecordOverviewRange
  start: string
  end: string
}

export function createUsageActions(deps: {
  guard: AsyncActionGuard
  currentPerspective: () => RequestOverviewPerspective
  createOverviewRequest: (
    perspective: RequestOverviewPerspective,
  ) => UsageOverviewRequest
  fetchOverview: (
    request: UsageOverviewRequest,
  ) => Promise<RequestRecordOverviewResponse | null>
  setOverview: (overview: RequestRecordOverviewResponse | null) => void
  setOverviewLoading: (loading: boolean) => void
  setPerspective: (perspective: RequestOverviewPerspective) => void
}) {
  const {
    createOverviewRequest,
    currentPerspective,
    fetchOverview,
    guard,
    setOverview,
    setOverviewLoading,
    setPerspective,
  } = deps
  let overviewLoadVersion = 0

  async function refreshOverview(
    request = createOverviewRequest(currentPerspective()),
  ): Promise<void> {
    const loadVersion = ++overviewLoadVersion
    setOverviewLoading(true)
    try {
      const overview = await fetchOverview(request)
      const current = createOverviewRequest(currentPerspective())
      if (
        overviewLoadVersion === loadVersion &&
        request.requestCategory === current.requestCategory &&
        request.perspective === current.perspective &&
        request.range === current.range &&
        request.start === current.start &&
        request.end === current.end
      ) {
        setOverview(overview)
      }
    } finally {
      if (overviewLoadVersion === loadVersion) setOverviewLoading(false)
    }
  }

  function setAiPerspective(
    nextPerspective: RequestOverviewPerspective,
  ): Promise<AsyncActionOutcome<void>> {
    if (currentPerspective() === nextPerspective) {
      return Promise.resolve({ executed: false })
    }
    const request = createOverviewRequest(nextPerspective)
    return guard.run(
      USAGE_PERSPECTIVE_KEY,
      async () => {
        setPerspective(nextPerspective)
        await refreshOverview(request)
      },
      'perspective',
    )
  }

  return {
    perspectivePending: computed(() => guard.isPending(USAGE_PERSPECTIVE_KEY)),
    refreshOverview,
    setAiPerspective,
  }
}
