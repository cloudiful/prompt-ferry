import { describe, expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
} from '../src/admin-mappers/forms/endpoint'
import {
  createEmptyModelRouteForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from '../src/admin-mappers/forms/model-route'
import {
  hasTargetSchedule,
  isTargetScheduleValid,
  sortedTargetWindows,
  targetScheduleSummary,
} from '../src/components/endpoints/modelRouteTargetHelpers'
import { useEndpointDialogValidation } from '../src/composables/useEndpointDialogValidation'
import type { EndpointForm, ModelRouteTargetForm } from '../src/models'
import * as scheduleWindows from '../src/models/endpoints/schedule-windows'

// Issue #1045: a schedule row with both times empty means "all day on the
// selected weekdays". The row stays blank in the editor, the API receives the
// canonical `00:00`-`24:00` window, and a partially filled row stays invalid.

type Row = { start: string; end: string; days?: number[] }

const t = (key: string) => key

describe('schedule row rules (#1045)', () => {
  test('a row with both times empty is valid and reports no error', () => {
    expect(scheduleWindows.isBlankScheduleRow({ start: '', end: '' })).toBe(
      true,
    )
    expect(scheduleWindows.scheduleRowError({ start: '', end: '' }, t)).toBe('')
    expect(scheduleWindows.isScheduleRowValid({ start: '', end: '' })).toBe(
      true,
    )
  })

  test('a partially filled row stays invalid', () => {
    expect(
      scheduleWindows.scheduleRowError({ start: '09:00', end: '' }, t),
    ).toBe('scheduleRequired')
    expect(
      scheduleWindows.scheduleRowError({ start: '', end: '17:00' }, t),
    ).toBe('scheduleRequired')
    expect(
      scheduleWindows.isScheduleRowValid({ start: '09:00', end: '' }),
    ).toBe(false)
  })

  test('a malformed or equal time pair stays invalid', () => {
    expect(
      scheduleWindows.scheduleRowError({ start: '9:00', end: '17:00' }, t),
    ).toBe('scheduleInvalid')
    expect(
      scheduleWindows.scheduleRowError({ start: '09:00', end: '24:30' }, t),
    ).toBe('scheduleInvalid')
    expect(
      scheduleWindows.scheduleRowError({ start: '09:00', end: '09:00' }, t),
    ).toBe('scheduleEqual')
    expect(
      scheduleWindows.isScheduleRowValid({ start: '09:00', end: '09:00' }),
    ).toBe(false)
  })

  test('a blank row normalizes to the canonical all-day window', () => {
    expect(
      scheduleWindows.normalizeScheduleWindowTimes({ start: '', end: '' }),
    ).toEqual({ start: '00:00', end: '24:00' })
    expect(
      scheduleWindows.normalizeScheduleWindowTimes({
        start: ' 09:00 ',
        end: '',
      }),
    ).toEqual({ start: '09:00', end: '' })
    expect(
      scheduleWindows.normalizeScheduleWindowTimes({
        start: '22:00',
        end: '06:00',
      }),
    ).toEqual({ start: '22:00', end: '06:00' })
  })
})

describe('endpoint save path (#1045)', () => {
  function formWith(windows: Row[], touched = true): EndpointForm {
    return {
      ...createEmptyEndpointForm(),
      active_windows: windows,
      active_windows_touched: touched,
    }
  }

  test('a blank endpoint row is sent as the canonical all-day window', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '', end: '', days: [1, 2, 3, 4, 5] }]),
    )

    expect(request.active_windows).toEqual([
      { start: '00:00', end: '24:00', days: [1, 2, 3, 4, 5] },
    ])
  })

  test('a blank endpoint row without weekdays stays all-day', () => {
    const request = endpointFormToRequest(formWith([{ start: '', end: '' }]))

    expect(request.active_windows).toEqual([{ start: '00:00', end: '24:00' }])
  })

  test('an untouched endpoint schedule is still omitted', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '', end: '', days: [1] }], false),
    )

    expect(request.active_windows).toBeUndefined()
  })

  test('an endpoint without windows is still sent as an empty array', () => {
    const request = endpointFormToRequest(formWith([]))

    expect(request.active_windows).toEqual([])
  })

  test('a partially filled endpoint row is never turned into all-day', () => {
    const request = endpointFormToRequest(
      formWith([{ start: '09:00', end: '' }]),
    )

    expect(request.active_windows).toEqual([{ start: '09:00', end: '' }])
  })

  test('an existing timed or overnight window is preserved', () => {
    const request = endpointFormToRequest(
      formWith([
        { start: '09:00', end: '17:00', days: [1] },
        { start: '22:00', end: '06:00', days: [6, 7] },
      ]),
    )

    expect(request.active_windows).toEqual([
      { start: '09:00', end: '17:00', days: [1] },
      { start: '22:00', end: '06:00', days: [6, 7] },
    ])
  })
})

describe('target save path (#1045)', () => {
  function formWith(windows: Row[], touched = true) {
    return {
      ...createEmptyModelRouteForm(),
      targets: [
        {
          ...createEmptyModelRouteForm().targets[0]!,
          endpoint_id: 'endpoint-1',
          active_windows: windows,
          active_windows_touched: touched,
        },
      ],
    }
  }

  test('a blank target row is sent as the canonical all-day window', () => {
    const request = modelRouteFormToRequest(
      formWith([{ start: '', end: '', days: [6, 7] }]),
    )

    expect(request.targets[0]?.active_windows).toEqual([
      { start: '00:00', end: '24:00', days: [6, 7] },
    ])
  })

  test('an untouched target schedule is still omitted', () => {
    const request = modelRouteFormToRequest(
      formWith([{ start: '', end: '', days: [1] }], false),
    )

    expect(request.targets[0]?.active_windows).toBeUndefined()
  })

  test('a target without windows is still sent as an empty array', () => {
    const request = modelRouteFormToRequest(formWith([]))

    expect(request.targets[0]?.active_windows).toEqual([])
  })

  test('a partially filled target row is never turned into all-day', () => {
    const request = modelRouteFormToRequest(
      formWith([{ start: '', end: '17:00' }]),
    )

    expect(request.targets[0]?.active_windows).toEqual([
      { start: '', end: '17:00' },
    ])
  })

  test('a saved all-day target row reopens as the canonical window', () => {
    const form = modelRouteToForm({
      targets: [
        {
          endpoint_id: 'endpoint-1',
          active_windows: [
            { start: '00:00', end: '24:00', days: [1, 2, 3, 4, 5] },
          ],
        },
      ],
    } as never)

    expect(form.targets[0]?.active_windows).toEqual([
      { start: '00:00', end: '24:00', days: [1, 2, 3, 4, 5] },
    ])

    const request = modelRouteFormToRequest({
      ...form,
      targets: form.targets.map((target) => ({
        ...target,
        active_windows_touched: true,
      })),
    })
    expect(request.targets[0]?.active_windows).toEqual([
      { start: '00:00', end: '24:00', days: [1, 2, 3, 4, 5] },
    ])
  })
})

describe('save gates (#1045)', () => {
  function targetWith(windows: Row[]): ModelRouteTargetForm {
    return {
      ...createEmptyModelRouteForm().targets[0]!,
      endpoint_id: 'endpoint-1',
      active_windows: windows,
    }
  }

  test('the target schedule gate accepts a blank row and rejects a partial row', () => {
    expect(isTargetScheduleValid(targetWith([{ start: '', end: '' }]))).toBe(
      true,
    )
    expect(
      isTargetScheduleValid(targetWith([{ start: '09:00', end: '' }])),
    ).toBe(false)
    expect(
      isTargetScheduleValid(targetWith([{ start: '09:00', end: '09:00' }])),
    ).toBe(false)
  })

  test('the endpoint dialog gate accepts a blank row and rejects a partial row', () => {
    const blank = vue.ref<EndpointForm>({
      ...createEmptyEndpointForm(),
      active_windows: [{ start: '', end: '' }],
      active_windows_touched: true,
    })
    expect(useEndpointDialogValidation(blank, t).canSaveOuter.value).toBe(true)

    const partial = vue.ref<EndpointForm>({
      ...createEmptyEndpointForm(),
      active_windows: [{ start: '09:00', end: '' }],
      active_windows_touched: true,
    })
    expect(useEndpointDialogValidation(partial, t).canSaveOuter.value).toBe(
      false,
    )
  })
})

describe('schedule summaries (#1045)', () => {
  function targetWith(windows: Row[]): ModelRouteTargetForm {
    return {
      ...createEmptyModelRouteForm().targets[0]!,
      endpoint_id: 'endpoint-1',
      active_windows: windows,
    }
  }

  test('a blank target row stays visible as an explicit all-day override', () => {
    const target = targetWith([{ start: '', end: '', days: [1, 2] }])

    expect(hasTargetSchedule(target)).toBe(true)
    expect(sortedTargetWindows(target)).toEqual([
      { start: '00:00', end: '24:00', days: [1, 2] },
    ])
    expect(targetScheduleSummary(target, t)).toContain('00:00–24:00')
  })

  test('a target without rows keeps the inherit semantics', () => {
    const target = targetWith([])

    expect(hasTargetSchedule(target)).toBe(false)
    expect(targetScheduleSummary(target, t)).toBe('')
  })

  test('a partially filled row stays out of the summary', () => {
    const target = targetWith([{ start: '09:00', end: '' }])

    expect(hasTargetSchedule(target)).toBe(false)
  })

  test('an endpoint all-day row keeps the settings gear lit', () => {
    const form = vue.ref<EndpointForm>({
      ...createEmptyEndpointForm(),
      active_windows: [{ start: '', end: '' }],
      active_windows_touched: true,
    })
    expect(useEndpointDialogValidation(form, t).hasEndpointSettings()).toBe(
      true,
    )

    const cleared = vue.ref<EndpointForm>({
      ...createEmptyEndpointForm(),
      active_windows: [],
      active_windows_touched: true,
    })
    expect(useEndpointDialogValidation(cleared, t).hasEndpointSettings()).toBe(
      false,
    )
  })
})

// --- Component harness ------------------------------------------------------
// The repo has no DOM test environment, so the editor is mounted through
// Vue's SSR renderer (rendering) and through a custom host renderer
// (interactions). The component under test is the real SFC.

type Component = Parameters<typeof h>[0]

function rewriteSfcImports(code: string): string {
  return code.replace(
    /import\s+([^;'"]+?)\s+from\s+(['"])([^'"]+)\2;?/g,
    (_match, clause: string, _quote: string, spec: string) => {
      if (clause.trim().startsWith('type ')) return ''
      const named = clause.match(/\{([^}]*)\}/)
      const fallback = clause
        .replace(/\{[^}]*\}/, '')
        .replace(/^,|,$/g, '')
        .trim()
      const statements: string[] = []
      if (fallback) {
        statements.push(
          `const ${fallback} = __resolve(${JSON.stringify(spec)});`,
        )
      }
      for (const part of named?.[1].split(',') ?? []) {
        const entry = part.trim()
        if (!entry || entry.startsWith('type ')) continue
        const [source, local = source] = entry.split(/\s+as\s+/)
        statements.push(
          `const ${local.trim()} = __resolve(${JSON.stringify(spec)})[${JSON.stringify(source.trim())}];`,
        )
      }
      return statements.join(' ')
    },
  )
}

function compileComponent(
  source: string,
  filename: string,
  resolve: (spec: string) => unknown,
): Component {
  const { descriptor } = parse(source, { filename })
  const script = compileScript(descriptor, {
    id: filename,
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(script.content).replace(/export default/, 'return'),
  )
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
}

function slotStub(name: string, slot = 'default') {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots[slot]?.())
    },
  }
}

const buttonStub = {
  name: 'UButton',
  inheritAttrs: false,
  setup(
    _props: unknown,
    {
      attrs,
      slots,
    }: {
      attrs: Record<string, unknown>
      slots: Record<string, (() => unknown) | undefined>
    },
  ) {
    return () => h('button', { ...attrs }, slots.default?.())
  },
}

const inputStub = {
  name: 'UInput',
  inheritAttrs: false,
  setup(_props: unknown, { attrs }: { attrs: Record<string, unknown> }) {
    return () => h('input', { ...attrs, value: attrs['model-value'] ?? '' })
  },
}

const scheduleFieldsSource = await Bun.file(
  new URL(
    '../src/components/shared/ScheduleWindowsFields.vue',
    import.meta.url,
  ),
).text()

const scheduleFieldsComponent = compileComponent(
  scheduleFieldsSource,
  'ScheduleWindowsFields.vue',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/models/endpoints/schedule-windows') return scheduleWindows
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

async function renderFields(windows: Row[], touched = false): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(scheduleFieldsComponent as never, {
        windows,
        touched,
        t,
      }),
  })
  app.component('UButton', buttonStub as never)
  app.component('UInput', inputStub as never)
  app.component('UIcon', slotStub('UIcon') as never)
  return renderToString(app)
}

describe('schedule editor rendering (#1045)', () => {
  test('a blank row renders no error', async () => {
    const html = await renderFields([{ start: '', end: '' }])

    expect(html).not.toContain('scheduleRequired')
    expect(html).toContain('scheduleAddWindow')
  })

  test('a partially filled row renders the required error', async () => {
    const html = await renderFields([{ start: '09:00', end: '' }])

    expect(html).toContain('scheduleRequired')
  })

  test('a malformed row renders the invalid error', async () => {
    const html = await renderFields([{ start: '9:00', end: '17:00' }])

    expect(html).toContain('scheduleInvalid')
  })

  test('the weekday toggles are rendered for every row', async () => {
    const html = await renderFields([{ start: '', end: '', days: [1] }])

    expect(html).toContain('weekdays')
    expect(html).toContain('everyDay')
    expect(html).toContain('weekday1')
    expect(html).toContain('weekday7')
  })

  test('a saved all-day row reopens with the canonical times', async () => {
    const html = await renderFields([{ start: '00:00', end: '24:00' }])

    expect(html).toContain('value="00:00"')
    expect(html).toContain('value="24:00"')
    expect(html).not.toContain('scheduleRequired')
  })
})

// --- Interaction harness ----------------------------------------------------

type HostNode = {
  type?: string
  text?: string
  props: Record<string, unknown>
  children: HostNode[]
  parent: HostNode | null
}

function hostOps() {
  const make = (type?: string, text?: string): HostNode => ({
    type,
    text,
    props: {},
    children: [],
    parent: null,
  })
  const detach = (child: HostNode) => {
    if (child.parent) {
      const at = child.parent.children.indexOf(child)
      if (at >= 0) child.parent.children.splice(at, 1)
      child.parent = null
    }
  }
  return {
    patchProp: (el: HostNode, key: string, _prev: unknown, next: unknown) => {
      el.props[key] = next
    },
    insert: (child: HostNode, parent: HostNode, anchor?: HostNode | null) => {
      detach(child)
      child.parent = parent
      const at = anchor ? parent.children.indexOf(anchor) : -1
      if (at >= 0) parent.children.splice(at, 0, child)
      else parent.children.push(child)
    },
    remove: (child: HostNode) => detach(child),
    createElement: (type: string) => make(type),
    createText: (text: string) => make(undefined, text),
    createComment: (text: string) => make(undefined, text),
    setText: (node: HostNode, text: string) => {
      node.text = text
    },
    setElementText: (node: HostNode, text: string) => {
      node.children = [make(undefined, text)]
    },
    parentNode: (child: HostNode) => child.parent,
    nextSibling: (child: HostNode) => {
      if (!child.parent) return null
      const at = child.parent.children.indexOf(child)
      return at >= 0 ? (child.parent.children[at + 1] ?? null) : null
    },
    querySelector: () => null,
    setScopeId: () => {},
    cloneNode: (child: HostNode) => child,
    insertStaticContent: () => null,
  }
}

function textOf(node: HostNode): string {
  if (node.text) return node.text
  return node.children.map(textOf).join('')
}

function collectButtons(node: HostNode, found: HostNode[] = []): HostNode[] {
  if (node.type === 'button') found.push(node)
  for (const child of node.children) collectButtons(child, found)
  return found
}

function buttonByLabel(root: HostNode, label: string): HostNode {
  const button = collectButtons(root).find(
    (candidate) => textOf(candidate).trim() === label,
  )
  if (!button) throw new Error(`no button labelled ${label}`)
  return button
}

async function settle(): Promise<void> {
  const { nextTick } = vue
  for (let index = 0; index < 4; index += 1) await Promise.resolve()
  await nextTick()
  await Promise.resolve()
}

function mountFields() {
  const windows = vue.ref<Row[]>([])
  const touched = vue.ref(false)
  const Root = vue.defineComponent({
    setup() {
      return () =>
        h(scheduleFieldsComponent as never, {
          windows: windows.value,
          'onUpdate:windows': (value: Row[]) => {
            windows.value = value
          },
          touched: touched.value,
          'onUpdate:touched': (value: boolean) => {
            touched.value = value
          },
          t,
        })
    },
  })
  const renderer = (
    vue as unknown as {
      createRenderer: (ops: unknown) => {
        createApp: (root: unknown) => {
          component: (name: string, stub: unknown) => void
          mount: (root: unknown) => void
        }
      }
    }
  ).createRenderer(hostOps())
  const app = renderer.createApp(Root)
  app.component('UButton', buttonStub)
  app.component('UInput', inputStub)
  app.component('UIcon', slotStub('UIcon'))
  const root: HostNode = { props: {}, children: [], parent: null }
  app.mount(root)
  return { windows, touched, root }
}

describe('schedule editor interaction (#1045)', () => {
  test('adding a window shows a blank row without an error', async () => {
    const { windows, touched, root } = mountFields()

    const add = buttonByLabel(root, 'scheduleAddWindow')
    ;(add.props.onClick as () => void)()
    await settle()

    expect(windows.value).toEqual([{ start: '', end: '' }])
    expect(touched.value).toBe(true)
    expect(textOf(root)).not.toContain('scheduleRequired')
  })

  test('selecting weekdays keeps the row blank and marks it touched', async () => {
    const { windows, touched, root } = mountFields()

    ;(buttonByLabel(root, 'scheduleAddWindow').props.onClick as () => void)()
    await settle()

    ;(buttonByLabel(root, 'weekdays').props.onClick as () => void)()
    await settle()

    expect(windows.value).toEqual([
      { start: '', end: '', days: [1, 2, 3, 4, 5] },
    ])
    expect(touched.value).toBe(true)
    expect(textOf(root)).not.toContain('scheduleRequired')
  })

  test('a partially filled row reports the required error', async () => {
    const { windows, root } = mountFields()

    ;(buttonByLabel(root, 'scheduleAddWindow').props.onClick as () => void)()
    await settle()

    windows.value[0]!.start = '09:00'
    await settle()

    expect(textOf(root)).toContain('scheduleRequired')
  })
})
