import { describe, expect, it } from 'vitest'
import {
  createEmptyModelRouteForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from './model-route'
import type {
  ModelEndpointRule,
  ModelRouteRequest,
} from '../../generated/admin-api'
import type { ModelRouteForm, ModelRouteTargetForm } from '../../models'

function targetWith(
  overrides: Partial<ModelRouteTargetForm> = {},
): ModelRouteTargetForm {
  const target = createEmptyModelRouteForm().targets[0]
  return {
    ...target,
    endpoint_id: 'endpoint-1',
    active_windows_touched: true,
    ...overrides,
  }
}

function formWith(targets: ModelRouteTargetForm[]): ModelRouteForm {
  return { ...createEmptyModelRouteForm(), targets }
}

function targetsOf(request: ModelRouteRequest) {
  return request.targets ?? []
}

describe('modelRouteFormToRequest target schedules', () => {
  it('omits the schedule while the target is untouched', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [{ start: '09:00', end: '17:00', days: [1] }],
          active_windows_touched: false,
        }),
      ]),
    )

    expect(targetsOf(request)[0]?.active_windows).toBeUndefined()
  })

  it('sends an empty array when touched with no windows (all-day)', () => {
    const request = modelRouteFormToRequest(
      formWith([targetWith({ active_windows_touched: true })]),
    )

    expect(targetsOf(request)[0]?.active_windows).toEqual([])
  })

  it('keeps restricted weekdays sorted and deduplicated', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [
            { start: '09:00', end: '17:00', days: [5, 1, 3, 3, 2] },
          ],
        }),
      ]),
    )

    expect(targetsOf(request)[0]?.active_windows).toEqual([
      { start: '09:00', end: '17:00', days: [1, 2, 3, 5] },
    ])
  })

  it('omits days when every weekday is present (all-day semantics)', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [
            { start: '00:00', end: '24:00', days: [7, 1, 2, 3, 4, 5, 6] },
          ],
        }),
      ]),
    )

    expect(targetsOf(request)[0]?.active_windows?.[0]).not.toHaveProperty(
      'days',
    )
  })

  it('drops non-integer and out-of-range weekdays', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [
            { start: '09:00', end: '17:00', days: [0, 8, -1, 2.5, 3] },
          ],
        }),
      ]),
    )

    expect(targetsOf(request)[0]?.active_windows?.[0]?.days).toEqual([3])
  })

  it('omits days when no valid weekday remains (all-day)', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [{ start: '09:00', end: '17:00', days: [0, 9] }],
        }),
      ]),
    )

    expect(targetsOf(request)[0]?.active_windows?.[0]).not.toHaveProperty(
      'days',
    )
  })

  it('ignores a non-array days value', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [
            {
              start: '09:00',
              end: '17:00',
              days: '1,2' as unknown as number[],
            },
          ],
        }),
      ]),
    )

    expect(targetsOf(request)[0]?.active_windows?.[0]).not.toHaveProperty(
      'days',
    )
  })

  it('trims start/end and preserves 24:00 instead of clamping it', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [{ start: ' 00:00 ', end: ' 24:00 ', days: [6, 7] }],
        }),
      ]),
    )

    expect(targetsOf(request)[0]?.active_windows).toEqual([
      { start: '00:00', end: '24:00', days: [6, 7] },
    ])
  })

  it('sorts windows by start then end', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          active_windows: [
            { start: '18:00', end: '24:00' },
            { start: '12:00', end: '14:00' },
            { start: '00:00', end: '09:00' },
            { start: '00:00', end: '24:00' },
          ],
        }),
      ]),
    )

    expect(
      targetsOf(request)[0]?.active_windows?.map(
        (window) => `${window.start}-${window.end}`,
      ),
    ).toEqual(['00:00-09:00', '00:00-24:00', '12:00-14:00', '18:00-24:00'])
  })

  it('tracks touched state per target', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({
          endpoint_id: 'endpoint-1',
          active_windows: [{ start: '09:00', end: '17:00', days: [1] }],
          active_windows_touched: false,
        }),
        targetWith({
          endpoint_id: 'endpoint-2',
          active_windows: [{ start: '18:00', end: '24:00', days: [6] }],
          active_windows_touched: true,
        }),
      ]),
    )

    expect(targetsOf(request).map((target) => target.endpoint_id)).toEqual([
      'endpoint-1',
      'endpoint-2',
    ])
    expect(targetsOf(request)[0]?.active_windows).toBeUndefined()
    expect(targetsOf(request)[1]?.active_windows).toEqual([
      { start: '18:00', end: '24:00', days: [6] },
    ])
  })

  it('drops target rows without an endpoint id', () => {
    const request = modelRouteFormToRequest(
      formWith([
        targetWith({ endpoint_id: '', active_windows_touched: true }),
        targetWith({ endpoint_id: 'endpoint-2' }),
      ]),
    )

    expect(targetsOf(request)).toHaveLength(1)
    expect(targetsOf(request)[0]?.endpoint_id).toBe('endpoint-2')
  })
})

describe('model route schedule round-trip (#514)', () => {
  it('copies stored weekdays into the target and keeps them on save', () => {
    const route = {
      targets: [
        {
          endpoint_id: 'endpoint-1',
          active_windows: [
            { start: '00:00', end: '09:00', days: [1, 2, 3, 4, 5] },
            { start: '00:00', end: '24:00', days: [6, 7] },
          ],
        },
      ],
    } as unknown as ModelEndpointRule

    const form = modelRouteToForm(route)

    expect(form.targets[0]?.active_windows).toEqual([
      { start: '00:00', end: '09:00', days: [1, 2, 3, 4, 5] },
      { start: '00:00', end: '24:00', days: [6, 7] },
    ])
    expect(form.targets[0]?.active_windows_touched).toBe(false)

    const request = modelRouteFormToRequest({
      ...form,
      targets: form.targets.map((target) => ({
        ...target,
        active_windows_touched: true,
      })),
    })

    expect(targetsOf(request)[0]?.active_windows).toEqual([
      { start: '00:00', end: '09:00', days: [1, 2, 3, 4, 5] },
      { start: '00:00', end: '24:00', days: [6, 7] },
    ])
  })

  it('tolerates a stored target without a days array', () => {
    const form = modelRouteToForm({
      targets: [{ endpoint_id: 'endpoint-1', active_windows: [{}] }],
    } as unknown as ModelEndpointRule)

    expect(form.targets[0]?.active_windows).toEqual([{ start: '', end: '' }])
  })
})
