import { expect, test } from 'bun:test'
import { computed, ref } from 'vue'
import { loadActionFactory } from './relay-actions.fixtures'

type CreateResult = {
  executed: boolean
  value?: { secret: string }
}

type PageActions = {
  openCreateKey: () => Promise<void>
  submitCreateKey: () => Promise<void>
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

async function createPageHarness(
  createKey: (label: string, ownerId: number) => Promise<CreateResult>,
) {
  const source = await Bun.file(
    new URL('../src/pages/ApiKeysPage.vue', import.meta.url),
  ).text()
  const handlers = source.slice(
    source.indexOf('async function openCreateKey('),
    source.indexOf('function toggleKeySecret('),
  )
  const keyLabel = ref('')
  const generatedSecret = ref('')
  const keyDialogVisible = ref(false)
  const createDialogGeneration = ref(0)
  const createDialogOwnerId = ref(0)
  const apiErrors: unknown[] = []
  const errors: string[] = []
  const successes: string[] = []
  const apiKeysStore = { selectedUserId: 0, createKey }
  const javascript = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    handlers,
  )
  const actions = new Function(
    'keyLabel',
    'generatedSecret',
    'keyDialogVisible',
    'createDialogGeneration',
    'createDialogOwnerId',
    'apiKeysStore',
    'notifyError',
    'notifySuccess',
    'notifyApiError',
    't',
    `${javascript}; return { openCreateKey, submitCreateKey }`,
  )(
    keyLabel,
    generatedSecret,
    keyDialogVisible,
    createDialogGeneration,
    createDialogOwnerId,
    apiKeysStore,
    (message: string) => errors.push(message),
    (message: string) => successes.push(message),
    (cause: unknown) => apiErrors.push(cause),
    (key: string) => key,
  ) as PageActions

  return {
    actions,
    apiErrors,
    apiKeysStore,
    createDialogGeneration,
    createDialogOwnerId,
    errors,
    generatedSecret,
    keyDialogVisible,
    keyLabel,
    successes,
  }
}

test('an old same-user create cannot populate a reopened key dialog', async () => {
  const pending = deferred<CreateResult>()
  const calls: Array<[string, number]> = []
  const harness = await createPageHarness((label, ownerId) => {
    calls.push([label, ownerId])
    return pending.promise
  })

  await harness.actions.openCreateKey()
  harness.keyLabel.value = 'first draft'
  const saving = harness.actions.submitCreateKey()
  await harness.actions.openCreateKey()
  harness.keyLabel.value = 'new draft'
  pending.resolve({ executed: true, value: { secret: 'old-dialog-secret' } })
  await saving

  expect(calls).toEqual([['first draft', 0]])
  expect(harness.keyDialogVisible.value).toBe(true)
  expect(harness.keyLabel.value).toBe('new draft')
  expect(harness.generatedSecret.value).toBe('')
  expect(harness.successes).toEqual([])
})

test('an old-owner completion cannot change a newly opened other-owner dialog', async () => {
  const pending = deferred<CreateResult>()
  const calls: Array<[string, number]> = []
  const harness = await createPageHarness((label, ownerId) => {
    calls.push([label, ownerId])
    return pending.promise
  })

  await harness.actions.openCreateKey()
  harness.keyLabel.value = 'owner zero draft'
  const saving = harness.actions.submitCreateKey()
  harness.apiKeysStore.selectedUserId = 7
  await harness.actions.openCreateKey()
  harness.keyLabel.value = 'owner seven draft'
  pending.resolve({ executed: true, value: { secret: 'old-owner-secret' } })
  await saving

  expect(calls).toEqual([['owner zero draft', 0]])
  expect(harness.createDialogOwnerId.value).toBe(7)
  expect(harness.keyDialogVisible.value).toBe(true)
  expect(harness.keyLabel.value).toBe('owner seven draft')
  expect(harness.generatedSecret.value).toBe('')
  expect(harness.successes).toEqual([])
})

test('the current create shows its captured owner secret once', async () => {
  const calls: Array<[string, number]> = []
  const harness = await createPageHarness(async (label, ownerId) => {
    calls.push([label, ownerId])
    return { executed: true, value: { secret: 'current-owner-secret' } }
  })
  harness.apiKeysStore.selectedUserId = 7
  await harness.actions.openCreateKey()
  harness.keyLabel.value = 'current draft'

  await harness.actions.submitCreateKey()

  expect(calls).toEqual([['current draft', 7]])
  expect(harness.generatedSecret.value).toBe('current-owner-secret')
  expect(harness.successes).toEqual(['saved'])
})

test('a late relay save cannot close and reset a newly opened relay draft', async () => {
  const pending = deferred<{ executed: true }>()
  const notices: string[] = []
  const createActions = await loadActionFactory<
    (options: Record<string, unknown>) => {
      dialogVisible: { value: boolean }
      dialogForm: { value: { relay_id: string; name: string } }
      openCreateDialog: () => void
      saveRelay: () => Promise<void>
    }
  >('../src/composables/useRelaysPageActions.ts', 'useRelaysPageActions', {
    computed,
    ref,
    createEmptyRelayForm: () => ({ relay_id: '', name: '' }),
    createRelayPatchRequest: (form: unknown) => form,
    createRelayRequest: (form: unknown) => form,
    relayToForm: (relay: unknown) => relay,
    useNotifier: () => ({
      notifyApiError: () => {},
      notifySuccess: (message: string) => notices.push(message),
    }),
    CREATE_KEY: 'create',
  })
  const actions = createActions({
    relayStore: {
      relays: [],
      relayPendingActions: {},
      saveRelay: () => pending.promise,
    },
    t: (key: string) => key,
  })
  actions.dialogVisible.value = true
  actions.dialogForm.value = { relay_id: 'relay-a', name: 'original draft' }
  const saving = actions.saveRelay()
  actions.dialogVisible.value = false
  actions.openCreateDialog()
  actions.dialogForm.value.name = 'new relay draft'
  pending.resolve({ executed: true })
  await saving

  expect(actions.dialogVisible.value).toBe(true)
  expect(actions.dialogForm.value.name).toBe('new relay draft')
  expect(notices).toEqual([])
})

test('a late host-settings save preserves the newer editable bind draft', async () => {
  const pending = deferred<{ executed: true }>()
  const createActions = await loadActionFactory<
    (options: Record<string, unknown>) => {
      saveSettings: () => Promise<void>
    }
  >(
    '../src/composables/useRelayHostPageActions.ts',
    'useRelayHostPageActions',
    {
      computed,
      ref,
      relayHostToForm: (host: unknown, settings: unknown) => ({
        role: (host as { role?: string } | null)?.role ?? 'integrated',
        admin_bind:
          (settings as { relay?: { admin_bind?: string } } | null)?.relay
            ?.admin_bind ?? '127.0.0.1:8790',
      }),
      useNotifier: () => ({
        notifyApiError: () => {},
        notifySuccess: () => {},
      }),
    },
  )
  const form = ref({
    role: 'integrated' as const,
    admin_bind: '127.0.0.1:8790',
  })
  const actions = createActions({
    form,
    hostStore: {
      host: { role: 'integrated' },
      settings: { relay: { admin_bind: '127.0.0.1:8790' } },
      relayHostPendingActions: { local: undefined },
      saveSettings: () => pending.promise,
    },
    relaySession: {},
    t: (key: string) => key,
    tokenInput: ref(''),
  })
  const saving = actions.saveSettings()
  form.value.admin_bind = '127.0.0.1:8800'
  pending.resolve({ executed: true })
  await saving

  expect(form.value.admin_bind).toBe('127.0.0.1:8800')
})

test('a failed current create retains its draft and can be retried', async () => {
  const failure = new Error('create rejected')
  let attempts = 0
  const harness = await createPageHarness(async () => {
    attempts += 1
    if (attempts === 1) throw failure
    return { executed: true, value: { secret: 'retry-secret' } }
  })
  await harness.actions.openCreateKey()
  harness.keyLabel.value = 'retry draft'

  await harness.actions.submitCreateKey()
  expect(harness.apiErrors).toEqual([failure])
  expect(harness.keyLabel.value).toBe('retry draft')
  expect(harness.generatedSecret.value).toBe('')

  await harness.actions.submitCreateKey()
  expect(attempts).toBe(2)
  expect(harness.generatedSecret.value).toBe('retry-secret')
  expect(harness.successes).toEqual(['saved'])
})
