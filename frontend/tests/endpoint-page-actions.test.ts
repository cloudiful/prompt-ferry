import { beforeEach, expect, test } from 'bun:test'

import type { EndpointRequest } from '../src/generated/admin-api'
import {
  deferred,
  freshPage,
  type Page,
} from './endpoint-page-actions.fixtures'

/**
 * The completion contract: what each operation reports, and what it may close.
 *
 * Every wrapper reports success only when the store says it ran, so a call the
 * guard refused produces no toast, no error, and no close. The dialog lifecycle
 * itself — generations, the pending binding, the headers — is covered in
 * `endpoint-page-actions.lifecycle.test.ts`.
 */

let page: Page

beforeEach(() => {
  page = freshPage()
})

test('a successful save reports once, closes once, and clears the draft', async () => {
  page.page.editEndpoint('endpoint-a')
  expect(page.page.endpointDialogVisible.value).toBe(true)

  await page.page.saveEndpoint()

  expect(page.store.saveEndpoint).toHaveBeenCalledTimes(1)
  expect(page.notices.successes).toEqual(['endpointSaved'])
  expect(page.notices.errors).toEqual([])
  expect(page.page.endpointDialogVisible.value).toBe(false)
  expect(page.page.endpointForm.value.endpoint_id).toBe('')
})

test('the save sends the draft as it stood when the button was pressed', async () => {
  page.page.editEndpoint('endpoint-a')

  await page.page.saveEndpoint()

  const [id, body] = page.store.saveEndpoint.mock.calls[0] as [
    string | null,
    EndpointRequest,
  ]
  expect(id).toBe('endpoint-a')
  expect(body.name).toBe('upstream endpoint-a')
})

test('a refused save leaves the dialog open and says nothing', async () => {
  page = freshPage({ executed: false })
  page.page.editEndpoint('endpoint-a')

  await page.page.saveEndpoint()

  expect(page.store.saveEndpoint).toHaveBeenCalledTimes(1)
  expect(page.notices.successes).toEqual([])
  expect(page.notices.errors).toEqual([])
  expect(page.page.endpointDialogVisible.value).toBe(true)
  expect(page.page.endpointForm.value.endpoint_id).toBe('endpoint-a')
})

test('three presses in a row report nothing at all', async () => {
  const gate = deferred()
  page.store.saveEndpoint.mockImplementation(async () => {
    await gate.promise
    return { executed: false }
  })
  page.page.editEndpoint('endpoint-a')

  // A held Enter and two clicks: the store's guard collapses them, and a call
  // it refused must not report a completion the page then acts on.
  const presses = [
    page.page.saveEndpoint(),
    page.page.saveEndpoint(),
    page.page.saveEndpoint(),
  ]
  gate.resolve()
  await Promise.all(presses)

  expect(page.notices.successes).toEqual([])
  expect(page.notices.errors).toEqual([])
  expect(page.page.endpointDialogVisible.value).toBe(true)
})

test('a failed save reports the error and keeps the draft for a retry', async () => {
  const boom = new Error('validation failed')
  page.store.saveEndpoint.mockImplementation(async () => {
    throw boom
  })
  page.page.editEndpoint('endpoint-a')
  page.page.endpointForm.value.name = 'renamed upstream'

  await page.page.saveEndpoint()

  expect(page.notices.errors).toEqual([boom])
  expect(page.notices.successes).toEqual([])
  expect(page.page.endpointDialogVisible.value).toBe(true)
  expect(page.page.endpointForm.value.name).toBe('renamed upstream')
  // A failure is not a completion, so the pending state has already cleared and
  // a retry is not blocked.
  expect(page.page.endpointSavePending.value).toBe(false)
})

test('a delete reports once, only once confirmed', async () => {
  await page.page.deleteEndpoint('endpoint-a')

  expect(page.confirmations).toEqual(['delete upstream endpoint-a?'])
  expect(page.store.removeEndpoint).toHaveBeenCalledTimes(1)
  expect(page.notices.successes).toEqual(['endpointDeleted'])
})

test('a cancelled or unresolvable delete makes no request and says nothing', async () => {
  page = freshPage({ confirm: false })

  await page.page.deleteEndpoint('endpoint-a')
  await page.page.deleteEndpoint('endpoint-missing')
  await page.page.deleteModelRoute('rule-a')

  expect(page.confirmations).toEqual([
    'delete upstream endpoint-a?',
    'delete model-rule-a?',
  ])
  // A record that is not loaded is never even confirmed, and a cancelled one
  // never reaches the store.
  expect(page.store.removeEndpoint).not.toHaveBeenCalled()
  expect(page.store.removeModelRoute).not.toHaveBeenCalled()
  expect(page.notices.successes).toEqual([])
  expect(page.notices.errors).toEqual([])
})

test('a refused delete reports nothing', async () => {
  page = freshPage({ executed: false })

  await page.page.deleteEndpoint('endpoint-a')

  expect(page.notices.successes).toEqual([])
  expect(page.notices.errors).toEqual([])
})

test('a failed delete reports the error once', async () => {
  const boom = new Error('delete rejected')
  page.store.removeEndpoint.mockImplementation(async () => {
    throw boom
  })

  await page.page.deleteEndpoint('endpoint-a')

  expect(page.notices.errors).toEqual([boom])
  expect(page.notices.successes).toEqual([])
})

test('toggles and probes pass through and report only a failure', async () => {
  const toggleBoom = new Error('toggle rejected')
  const probeBoom = new Error('probe rejected')

  await page.page.toggleEndpointEnabled('endpoint-a', false)
  await page.page.testEndpoint('endpoint-a')
  await page.page.toggleModelRouteEnabled('rule-a', false)
  await page.page.testModelRoute('rule-a')

  expect(page.store.toggleEndpointEnabled).toHaveBeenCalledWith(
    'endpoint-a',
    false,
  )
  expect(page.store.runEndpointTest).toHaveBeenCalledWith('endpoint-a')
  expect(page.store.toggleModelRouteEnabled).toHaveBeenCalledWith(
    'rule-a',
    false,
  )
  expect(page.store.runModelRouteTest).toHaveBeenCalledWith('rule-a')
  expect(page.notices.errors).toEqual([])
  expect(page.notices.successes).toEqual([])

  page.store.toggleEndpointEnabled.mockImplementation(async () => {
    throw toggleBoom
  })
  page.store.runEndpointTest.mockImplementation(async () => {
    throw probeBoom
  })
  await page.page.toggleEndpointEnabled('endpoint-a', true)
  await page.page.testEndpoint('endpoint-a')

  expect(page.notices.errors).toEqual([toggleBoom, probeBoom])
})

test('a successful model route save reports once and clears the draft', async () => {
  page.page.editModelRoute('rule-a')

  await page.page.saveModelRoute()

  expect(page.store.saveModelRoute).toHaveBeenCalledTimes(1)
  expect(page.notices.successes).toEqual(['saved'])
  expect(page.page.modelRouteDialogVisible.value).toBe(false)
  expect(page.page.modelRouteForm.value.rule_id).toBe('')
})

test('a refused or failed model route save keeps the dialog open', async () => {
  page = freshPage({ executed: false })
  page.page.editModelRoute('rule-a')
  await page.page.saveModelRoute()

  expect(page.notices.successes).toEqual([])
  expect(page.page.modelRouteDialogVisible.value).toBe(true)

  const boom = new Error('route rejected')
  page = freshPage()
  page.store.saveModelRoute.mockImplementation(async () => {
    throw boom
  })
  page.page.editModelRoute('rule-a')
  await page.page.saveModelRoute()

  expect(page.notices.errors).toEqual([boom])
  expect(page.page.modelRouteDialogVisible.value).toBe(true)
  expect(page.page.modelRouteForm.value.rule_id).toBe('rule-a')
})
