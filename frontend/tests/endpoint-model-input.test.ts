import { expect, mock, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h, nextTick, ref } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type { EndpointModelsResponse } from '../src/generated/admin-api'
import * as endpointsApi from '../src/stores/endpoints-api'

// Issue #993 P1: the upstream model override is a searchable control over the
// endpoint's own model listing. The listing only offers suggestions: a custom
// identifier stays editable, a blank value still means "inherit", and a failed
// listing never clears the draft.

const fetchEndpointModels = mock(
  async (_endpointId: string): Promise<EndpointModelsResponse> => ({
    models: [],
  }),
)

mock.module('../src/stores/endpoints-api', () => ({
  ...endpointsApi,
  fetchEndpointModels,
}))

const { useEndpointModels } =
  await import('../src/composables/useEndpointModels')

async function flush(): Promise<void> {
  await nextTick()
  await new Promise((resolve) => setTimeout(resolve, 0))
  await nextTick()
}

test('the listing is deduplicated and cached per endpoint', async () => {
  fetchEndpointModels.mockClear()
  fetchEndpointModels.mockImplementation(async (endpointId: string) => ({
    models: ['gpt-4o-mini', 'gpt-4o', 'gpt-4o-mini'],
  }))

  const first = useEndpointModels(ref('endpoint-dedup'))
  await flush()
  expect(first.models.value).toEqual(['gpt-4o', 'gpt-4o-mini'])
  expect(first.loading.value).toBe(false)
  expect(first.errorMessage.value).toBe('')
  expect(fetchEndpointModels).toHaveBeenCalledTimes(1)

  // A second control for the same endpoint reuses the cached listing.
  const second = useEndpointModels(ref('endpoint-dedup'))
  await flush()
  expect(second.models.value).toEqual(['gpt-4o', 'gpt-4o-mini'])
  expect(fetchEndpointModels).toHaveBeenCalledTimes(1)
})

test('a concurrent read of the same endpoint shares one request', async () => {
  fetchEndpointModels.mockClear()
  let release: (value: EndpointModelsResponse) => void = () => {}
  fetchEndpointModels.mockImplementation(
    () =>
      new Promise<EndpointModelsResponse>((resolve) => {
        release = resolve
      }),
  )

  const first = useEndpointModels(ref('endpoint-shared'))
  const second = useEndpointModels(ref('endpoint-shared'))
  expect(fetchEndpointModels).toHaveBeenCalledTimes(1)

  release({ models: ['b-model', 'a-model'] })
  await flush()
  expect(first.models.value).toEqual(['a-model', 'b-model'])
  expect(second.models.value).toEqual(['a-model', 'b-model'])
})

test('a late answer for a replaced endpoint never populates the new one', async () => {
  fetchEndpointModels.mockClear()
  const releases: Record<string, (value: EndpointModelsResponse) => void> = {}
  fetchEndpointModels.mockImplementation(
    (endpointId: string) =>
      new Promise<EndpointModelsResponse>((resolve) => {
        releases[endpointId] = resolve
      }),
  )

  const endpointId = ref('endpoint-old')
  const models = useEndpointModels(endpointId)
  endpointId.value = 'endpoint-new'
  await flush()

  releases['endpoint-old']?.({ models: ['old-model'] })
  releases['endpoint-new']?.({ models: ['new-model'] })
  await flush()

  expect(models.models.value).toEqual(['new-model'])
})

test('switching endpoints clears the previous suggestions while the new listing loads', async () => {
  fetchEndpointModels.mockClear()
  fetchEndpointModels.mockImplementationOnce(async () => ({
    models: ['only-on-old-endpoint'],
  }))
  const endpointId = ref('reviewer-loaded-old')
  const listing = useEndpointModels(endpointId)
  await flush()
  expect(listing.models.value).toEqual(['only-on-old-endpoint'])

  let release: (value: EndpointModelsResponse) => void = () => {}
  fetchEndpointModels.mockImplementationOnce(
    () =>
      new Promise<EndpointModelsResponse>((resolve) => {
        release = resolve
      }),
  )
  endpointId.value = 'reviewer-pending-new'
  await flush()
  const suggestionsWhileLoading = [...listing.models.value]
  const loading = listing.loading.value
  release({ models: ['only-on-new-endpoint'] })
  await flush()
  expect(loading).toBe(true)
  expect(listing.models.value).toEqual(['only-on-new-endpoint'])
  expect(suggestionsWhileLoading).toEqual([])
})

test('switching to a cached endpoint shows its suggestions without a new read', async () => {
  fetchEndpointModels.mockClear()
  fetchEndpointModels.mockImplementation(async (endpointId: string) => ({
    models: [`only-on-${endpointId}`],
  }))

  const endpointId = ref('cached-old')
  const listing = useEndpointModels(endpointId)
  await flush()
  expect(listing.models.value).toEqual(['only-on-cached-old'])

  endpointId.value = 'cached-new'
  await flush()
  expect(listing.models.value).toEqual(['only-on-cached-new'])

  // The first endpoint is cached now, so switching back answers from the
  // cache and never offers the other endpoint's suggestions.
  endpointId.value = 'cached-old'
  await flush()
  expect(listing.models.value).toEqual(['only-on-cached-old'])
  expect(fetchEndpointModels).toHaveBeenCalledTimes(2)
})

test('a failed listing is retryable and never cached', async () => {
  fetchEndpointModels.mockClear()
  fetchEndpointModels.mockImplementationOnce(async () => {
    throw new Error('model discovery unavailable')
  })

  const models = useEndpointModels(ref('endpoint-retry'))
  await flush()
  expect(models.errorMessage.value).toContain('model discovery unavailable')
  expect(models.loading.value).toBe(false)
  expect(models.models.value).toEqual([])

  fetchEndpointModels.mockImplementation(async () => ({
    models: ['recovered-model'],
  }))
  await models.refresh()
  expect(models.errorMessage.value).toBe('')
  expect(models.models.value).toEqual(['recovered-model'])
  expect(fetchEndpointModels).toHaveBeenCalledTimes(2)
})

test('an endpoint without an id lists nothing and never asks the API', async () => {
  fetchEndpointModels.mockClear()
  const models = useEndpointModels(ref(''))
  await flush()
  expect(fetchEndpointModels).toHaveBeenCalledTimes(0)
  expect(models.models.value).toEqual([])
  expect(models.loading.value).toBe(false)
  expect(models.errorMessage.value).toBe('')
})

// The control itself: the free text stays editable and the listing only
// feeds the suggestions.
type Component = Parameters<typeof h>[0]

function rewriteSfcImports(code: string): string {
  return code.replace(
    /import\s+([^;'"]+?)\s+from\s+(['"])([^'"]+)\2;?/g,
    (_match, clause: string, _quote: string, spec: string) => {
      if (clause.trim().startsWith('type ')) return ''
      const statements: string[] = []
      const named = clause.match(/\{([^}]*)\}/)
      const defaults = clause
        .replace(/\{[^}]*\}/, '')
        .split(',')
        .map((part) => part.trim())
        .filter((part) => part && !part.startsWith('type '))
      for (const fallback of defaults) {
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

const inputSource = await Bun.file(
  new URL(
    '../src/components/endpoints/EndpointModelInput.vue',
    import.meta.url,
  ),
).text()

type Recorded = {
  createItem: unknown
  disabled: unknown
  items: unknown
  loading: unknown
  mode: unknown
  modelValue: unknown
  placeholder: unknown
}

const recorded: Recorded[] = []
let emit: ((event: string, ...args: unknown[]) => void) | null = null

const inputMenuStub = {
  name: 'UInputMenu',
  inheritAttrs: false,
  props: {
    modelValue: String,
    items: Array,
    mode: String,
    createItem: [Boolean, String, Object],
    loading: Boolean,
    placeholder: String,
    disabled: Boolean,
  },
  setup(props: Record<string, unknown>, context: { emit: typeof emit }) {
    emit = context.emit
    return () => {
      recorded.push(props as unknown as Recorded)
      return h('input', { value: props.modelValue })
    }
  },
}

const listing = {
  errorMessage: vue.ref(''),
  loading: vue.ref(false),
  models: vue.ref<string[]>(['gpt-4o-mini', 'gpt-4o']),
}

const inputComponent: Component = compileComponent(
  inputSource,
  'EndpointModelInput.vue',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useEndpointModels') {
      return { useEndpointModels: () => listing }
    }
    return { default: { render: () => h('div') } }
  },
)

async function renderInput(
  value: string,
): Promise<{ html: string; model: { value: string } }> {
  const model = ref(value)
  const app = createSSRApp({
    render: () =>
      h(inputComponent, {
        modelValue: model.value,
        'onUpdate:modelValue': (next: string) => {
          model.value = next
        },
        endpointId: 'endpoint-1',
        placeholder: 'upstream model',
        t: (key: string) => key,
      }),
  })
  app.component('UInputMenu', inputMenuStub)
  const html = await renderToString(app)
  return { html, model }
}

test('the control offers the endpoint listing as suggestions', async () => {
  recorded.length = 0
  const { html } = await renderInput('gpt-4o')

  const control = recorded[0]
  expect(control?.mode).toBe('autocomplete')
  expect(control?.createItem).toBe(true)
  expect(control?.items).toEqual(['gpt-4o-mini', 'gpt-4o'])
  expect(control?.modelValue).toBe('gpt-4o')
  expect(control?.placeholder).toBe('upstream model')
  // The custom text stays in the input, so the control never rejects it.
  expect(html).toContain('value="gpt-4o"')
})

test('a failed listing keeps the custom text editable', async () => {
  recorded.length = 0
  listing.errorMessage.value = 'model_discovery_failed'
  const { html } = await renderInput('my-own-model')

  const control = recorded[0]
  expect(control?.modelValue).toBe('my-own-model')
  expect(control?.disabled).toBeFalsy()
  expect(html).toContain('upstreamModelSuggestionsFailed')
  listing.errorMessage.value = ''
})

test('clearing the control empties the value', async () => {
  recorded.length = 0
  const { model } = await renderInput('gpt-4o')
  expect(model.value).toBe('gpt-4o')

  emit?.('clear')
  expect(model.value).toBe('')
})

// ---------------------------------------------------------------------------
// Issue #1032: the effective target protocol scopes the suggestion catalog. The
// consumer passes it through, and the cache is protocol-distinct for one
// endpoint.
// ---------------------------------------------------------------------------

type SelectorCapture = {
  endpointId: { value: string | null | undefined } | null
  nativeApi: { value: string | null | undefined } | null
}

const selectorCapture: SelectorCapture = { endpointId: null, nativeApi: null }

const inputComponentWithSelectorCapture: Component = compileComponent(
  inputSource,
  'EndpointModelInput.vue',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useEndpointModels') {
      return {
        useEndpointModels: (
          endpointId: { value: string | null | undefined },
          nativeApi?: { value: string | null | undefined },
        ) => {
          selectorCapture.endpointId = endpointId
          selectorCapture.nativeApi = nativeApi ?? null
          return listing
        },
      }
    }
    return { default: { render: () => h('div') } }
  },
)

async function renderInputWithSelector(
  nativeApi: string | null,
): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(inputComponentWithSelectorCapture, {
        modelValue: 'gpt-4o',
        endpointId: 'endpoint-1',
        nativeApi,
        placeholder: 'upstream model',
        t: (key: string) => key,
      }),
  })
  app.component('UInputMenu', inputMenuStub)
  return renderToString(app)
}

test('the control forwards the effective protocol as the catalog selector', async () => {
  selectorCapture.nativeApi = null
  await renderInputWithSelector('embeddings')
  expect(selectorCapture.endpointId?.value).toBe('endpoint-1')
  expect(selectorCapture.nativeApi?.value).toBe('embeddings')
})

test('an omitted protocol leaves the endpoint catalog unscoped', async () => {
  selectorCapture.nativeApi = null
  await renderInputWithSelector(null)
  expect(selectorCapture.nativeApi?.value).toBeNull()
})

test('the listing cache is protocol-distinct for one endpoint', async () => {
  fetchEndpointModels.mockClear()
  fetchEndpointModels.mockImplementation(async () => ({
    models: ['shared-model'],
  }))

  const endpointId = ref('endpoint-protocol')
  const selector = ref<'chat' | 'embeddings' | null>('chat')
  const selection = useEndpointModels(endpointId, selector)
  await flush()
  expect(selection.models.value).toEqual(['shared-model'])
  expect(fetchEndpointModels).toHaveBeenCalledTimes(1)
  expect(fetchEndpointModels.mock.calls[0]?.[1]).toBe('chat')

  // The selector is part of the cache key: a different protocol reads again.
  selector.value = 'embeddings'
  await flush()
  expect(fetchEndpointModels).toHaveBeenCalledTimes(2)
  expect(fetchEndpointModels.mock.calls[1]?.[1]).toBe('embeddings')

  // Repeating a protocol is served from its own cached entry.
  selector.value = 'chat'
  await flush()
  expect(fetchEndpointModels).toHaveBeenCalledTimes(2)

  // Back to the endpoint's own semantics (`null`) is yet another entry.
  selector.value = null
  await flush()
  expect(fetchEndpointModels).toHaveBeenCalledTimes(3)
  expect(fetchEndpointModels.mock.calls[2]?.[1]).toBeNull()
})

test('a protocol change isolates the previous list and keeps the latest result', async () => {
  fetchEndpointModels.mockClear()
  const releases: Record<string, (value: EndpointModelsResponse) => void> = {}
  fetchEndpointModels.mockImplementation(
    (_endpointId: string, nativeApi?: string | null) =>
      new Promise<EndpointModelsResponse>((resolve) => {
        releases[String(nativeApi ?? 'auto')] = resolve
      }),
  )

  const endpointId = ref('endpoint-latest')
  const selector = ref<'chat' | 'embeddings' | null>('chat')
  const selection = useEndpointModels(endpointId, selector)

  // The protocol changes before the chat answer arrives: the chat list must
  // never populate the embeddings control.
  selector.value = 'embeddings'
  await flush()

  releases['embeddings']?.({ models: ['embedding-model'] })
  await flush()
  expect(selection.models.value).toEqual(['embedding-model'])

  // A late chat answer is dropped.
  releases['chat']?.({ models: ['chat-model'] })
  await flush()
  expect(selection.models.value).toEqual(['embedding-model'])
})
