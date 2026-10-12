import { beforeEach, expect, test } from 'bun:test'

import {
  deferred,
  freshPage,
  type Page,
} from './endpoint-page-actions.fixtures'

/**
 * The dialog lifecycle: which surface a dialog is, and which surface a late
 * save is allowed to touch.
 *
 * Each dialog is identified by the generation it was opened in, so a save that
 * settles after the user has moved on reports its write but leaves the draft
 * alone. The pending binding that freezes each dialog is also a lifecycle
 * question — it follows the record the draft belongs to.
 */

let page: Page

beforeEach(() => {
  page = freshPage()
})

test('a save that lands on a reopened dialog never closes it', async () => {
  for (const reopen of [
    () => page.page.editEndpoint('endpoint-b'),
    () => page.page.editEndpoint('endpoint-a'),
    () => page.page.openEndpointDialog(),
  ]) {
    const gate = deferred()
    page = freshPage()
    page.page.editEndpoint('endpoint-a')
    const saving = page.page.saveEndpoint()

    reopen()
    gate.resolve()
    await saving

    // The draft on screen belongs to whoever opened it last, so it survives —
    // including when they reopened the very same record.
    expect(page.page.endpointDialogVisible.value).toBe(true)
    // The write itself did happen, so it is reported once; only the close and
    // the draft reset are withheld.
    expect(page.notices.successes).toEqual(['endpointSaved'])
  }
})

test('a model route save that lands on a newer dialog does not close it', async () => {
  const gate = deferred()
  page.page.editModelRoute('rule-a')
  const saving = page.page.saveModelRoute()

  page.page.editModelRoute('rule-b')

  gate.resolve()
  await saving

  expect(page.page.modelRouteDialogVisible.value).toBe(true)
  expect(page.page.modelRouteForm.value.rule_id).toBe('rule-b')
})

test('each dialog spinner follows the record its own draft belongs to', () => {
  page.page.editEndpoint('endpoint-a')
  expect(page.page.endpointSavePending.value).toBe(false)

  page.pending.endpoint = { 'endpoint-a': 'save' }
  expect(page.page.endpointSavePending.value).toBe(true)

  // A write on another record is not this dialog's. A delete of the same record
  // still freezes it, because that record is being written either way.
  page.pending.endpoint = { 'endpoint-b': 'save' }
  expect(page.page.endpointSavePending.value).toBe(false)
  page.pending.endpoint = { 'endpoint-a': 'delete' }
  expect(page.page.endpointSavePending.value).toBe(true)

  // A probe of the same record is not a write and never freezes the dialog.
  page.page.openEndpointDialog()
  expect(page.page.endpointSavePending.value).toBe(false)

  // An unsaved draft is gated under the same `create` id the row map names.
  page.pending.endpoint = { create: 'save' }
  expect(page.page.endpointSavePending.value).toBe(true)

  page.page.editModelRoute('rule-a')
  page.pending.modelRoute = { 'rule-a': 'save' }
  expect(page.page.modelRouteSavePending.value).toBe(true)
  page.pending.modelRoute = { 'rule-a': 'delete' }
  expect(page.page.modelRouteSavePending.value).toBe(true)
  page.pending.modelRoute = { create: 'save' }
  expect(page.page.modelRouteSavePending.value).toBe(false)
})

test('editing a record that is not loaded opens nothing', () => {
  page.page.editEndpoint('endpoint-missing')

  expect(page.page.endpointDialogVisible.value).toBe(false)
})

test('the dialog header names the mode the draft is in', () => {
  page.page.openEndpointDialog()
  expect(page.page.endpointDialogHeader.value).toBe('newEndpoint')
  page.page.editEndpoint('endpoint-a')
  expect(page.page.endpointDialogHeader.value).toBe('editEndpoint')

  page.page.openModelRouteDialog()
  expect(page.page.modelRouteDialogHeader.value).toBe('newModelRoute')
  page.page.editModelRoute('rule-a')
  expect(page.page.modelRouteDialogHeader.value).toBe('editModelRoute')
})
