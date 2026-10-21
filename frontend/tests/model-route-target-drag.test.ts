import { expect, test } from 'bun:test'
import { ref } from 'vue'
import { createEmptyModelRouteForm } from '../src/admin-mappers/forms/model-route'
import { useModelRouteTargetDrag } from '../src/components/endpoints/useModelRouteTargetDrag'

// Issue #993 P1: only the grip starts a row drag. A draggable ancestor around
// the cells made the browser start an HTML5 drag instead of the input's own
// text selection, so the caret and the selection were lost.

function formWithTargets(count: number) {
  const form = createEmptyModelRouteForm()
  const template = form.targets[0]
  if (!template) throw new Error('expected a default target')
  form.targets = Array.from({ length: count }, (_, index) => ({
    ...template,
    endpoint_id: `endpoint-${index}`,
  }))
  return form
}

function keyboardEvent(key: string): KeyboardEvent {
  return { key, preventDefault: () => {} } as unknown as KeyboardEvent
}

function dragEvent(target: unknown): DragEvent {
  return {
    target,
    preventDefault: () => {},
    dataTransfer: null,
  } as unknown as DragEvent
}

test('the grip reorders the targets with the keyboard', () => {
  const form = ref(formWithTargets(3))
  const { onGripKeydown } = useModelRouteTargetDrag(form)

  onGripKeydown(keyboardEvent('ArrowUp'), 2)
  expect(form.value.targets.map((target) => target.endpoint_id)).toEqual([
    'endpoint-0',
    'endpoint-2',
    'endpoint-1',
  ])

  onGripKeydown(keyboardEvent('ArrowDown'), 0)
  expect(form.value.targets.map((target) => target.endpoint_id)).toEqual([
    'endpoint-2',
    'endpoint-0',
    'endpoint-1',
  ])
})

test('a drag that does not start on the grip never moves a target', () => {
  const form = ref(formWithTargets(2))
  const { onRowDragStart, onDrop } = useModelRouteTargetDrag(form)

  let prevented = false
  const outside = {
    target: { closest: () => null },
    preventDefault: () => {
      prevented = true
    },
    dataTransfer: null,
  }
  onRowDragStart(outside as unknown as DragEvent, 0)
  expect(prevented).toBe(true)

  onDrop(
    {
      preventDefault: () => {},
      dataTransfer: { getData: () => '' },
    } as unknown as DragEvent,
    1,
  )
  expect(form.value.targets.map((target) => target.endpoint_id)).toEqual([
    'endpoint-0',
    'endpoint-1',
  ])
})

test('a drag that starts on the grip moves the target', () => {
  const form = ref(formWithTargets(2))
  const { onRowDragStart, onDrop } = useModelRouteTargetDrag(form)

  const grip = { target: { closest: () => ({}) }, dataTransfer: null }
  onRowDragStart(grip as unknown as DragEvent, 0)
  onDrop(
    {
      preventDefault: () => {},
      dataTransfer: { getData: () => '0' },
    } as unknown as DragEvent,
    1,
  )
  expect(form.value.targets.map((target) => target.endpoint_id)).toEqual([
    'endpoint-1',
    'endpoint-0',
  ])
})

const dialogSource = await Bun.file(
  new URL('../src/components/endpoints/ModelRouteDialog.vue', import.meta.url),
).text()

test('only the drag handle starts a row drag', () => {
  const draggables = dialogSource.match(/draggable="true"/g) ?? []
  expect(draggables.length).toBe(1)

  const handleIndex = dialogSource.indexOf('data-target-drag-handle')
  const draggableIndex = dialogSource.indexOf('draggable="true"')
  expect(handleIndex).toBeGreaterThan(-1)
  expect(draggableIndex).toBeGreaterThan(handleIndex)
  expect(draggableIndex - handleIndex).toBeLessThan(200)
})

test('the row cells stay drop targets and the grip keeps the keyboard reorder', () => {
  expect(
    dialogSource.match(/@dragover="onRowDragOver\(\$event, row\.index\)"/g)
      ?.length,
  ).toBe(4)
  expect(
    dialogSource.match(/@drop="onDrop\(\$event, row\.index\)"/g)?.length,
  ).toBe(4)
  expect(dialogSource).toContain(
    '@dragstart="onRowDragStart($event, row.index)"',
  )
  expect(dialogSource).toContain('@keydown="onGripKeydown($event, row.index)"')
})

test('the upstream model override uses the searchable control', () => {
  expect(dialogSource).toContain('<EndpointModelInput')
  expect(dialogSource).toContain('v-model="row.original.upstream_model"')
  expect(dialogSource).toContain(':endpoint-id="row.original.endpoint_id"')
})
