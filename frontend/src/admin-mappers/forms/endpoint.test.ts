import { describe, expect, it } from 'vitest'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
} from './endpoint'
import type { ProviderEndpoint } from '../../generated/admin-api'
import type { EndpointForm } from '../../models'

type Window = EndpointForm['active_windows'][number]

function formWith(windows: Window[], touched = true): EndpointForm {
  return {
    ...createEmptyEndpointForm(),
    active_windows: windows,
    active_windows_touched: touched,
  }
}

describe('endpointFormToRequest schedules', () => {
  it('omits active_windows while untouched so a PATCH keeps the stored schedule', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '09:00', end: '17:00', days: [1] }], false),
    )

    expect(request.active_windows).toBeUndefined()
  })

  it('sends an empty array when touched with no windows (all-day)', () => {
    const request = endpointFormToRequest(formWith([]))

    expect(request.active_windows).toEqual([])
  })

  it('keeps restricted weekdays sorted and deduplicated', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '09:00', end: '17:00', days: [5, 1, 3, 3, 2] }]),
    )

    expect(request.active_windows).toEqual([
      { start: '09:00', end: '17:00', days: [1, 2, 3, 5] },
    ])
  })

  it('omits days when every weekday is present (all-day semantics)', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '00:00', end: '24:00', days: [7, 1, 2, 3, 4, 5, 6] }]),
    )

    expect(request.active_windows?.[0]).toEqual({
      start: '00:00',
      end: '24:00',
    })
    expect(request.active_windows?.[0]).not.toHaveProperty('days')
  })

  it('drops non-integer and out-of-range weekdays', () => {
    const request = endpointFormToRequest(
      formWith([
        { start: '09:00', end: '17:00', days: [0, 8, -1, 2.5, Number.NaN, 3] },
      ]),
    )

    expect(request.active_windows?.[0]?.days).toEqual([3])
  })

  it('omits days when no valid weekday remains (all-day)', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '09:00', end: '17:00', days: [0, 9] }]),
    )

    expect(request.active_windows?.[0]).not.toHaveProperty('days')
  })

  it('keeps windows without a days array (legacy all-day)', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '09:00', end: '17:00' }]),
    )

    expect(request.active_windows?.[0]).toEqual({
      start: '09:00',
      end: '17:00',
    })
  })

  it('ignores a non-array days value', () => {
    const request = endpointFormToRequest(
      formWith([
        { start: '09:00', end: '17:00', days: '1,2' as unknown as number[] },
      ]),
    )

    expect(request.active_windows?.[0]).not.toHaveProperty('days')
  })

  it('trims start/end and preserves 24:00 instead of clamping it', () => {
    const request = endpointFormToRequest(
      formWith([{ start: ' 00:00 ', end: ' 24:00 ', days: [6, 7] }]),
    )

    expect(request.active_windows).toEqual([
      { start: '00:00', end: '24:00', days: [6, 7] },
    ])
  })

  it('sorts windows by start then end', () => {
    const request = endpointFormToRequest(
      formWith([
        { start: '18:00', end: '24:00' },
        { start: '12:00', end: '14:00' },
        { start: '00:00', end: '09:00' },
        { start: '00:00', end: '24:00' },
      ]),
    )

    expect(
      request.active_windows?.map((window) => `${window.start}-${window.end}`),
    ).toEqual(['00:00-09:00', '00:00-24:00', '12:00-14:00', '18:00-24:00'])
  })
})

describe('endpoint schedule round-trip (#514)', () => {
  it('copies stored weekdays into the form and keeps them on save', () => {
    const endpoint = {
      active_windows: [
        { start: '00:00', end: '09:00', days: [1, 2, 3, 4, 5] },
        { start: '00:00', end: '24:00', days: [6, 7] },
      ],
    } as unknown as ProviderEndpoint

    const form = endpointToForm(endpoint)

    expect(form.active_windows).toEqual([
      { start: '00:00', end: '09:00', days: [1, 2, 3, 4, 5] },
      { start: '00:00', end: '24:00', days: [6, 7] },
    ])
    expect(form.active_windows_touched).toBe(false)

    const request = endpointFormToRequest({
      ...form,
      active_windows_touched: true,
    })

    expect(request.active_windows).toEqual([
      { start: '00:00', end: '09:00', days: [1, 2, 3, 4, 5] },
      { start: '00:00', end: '24:00', days: [6, 7] },
    ])
  })

  it('tolerates a stored window without days', () => {
    const form = endpointToForm({
      active_windows: [{ start: '09:00', end: '17:00' }],
    } as unknown as ProviderEndpoint)

    expect(form.active_windows).toEqual([{ start: '09:00', end: '17:00' }])
  })
})
