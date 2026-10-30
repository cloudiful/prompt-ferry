import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter, Route, Routes } from 'react-router'
import { AppShell } from '@/layouts/AppShell'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RawObjectStoreSettingsResponse } from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  getRawObjectStore: vi.fn(),
  setRawObjectStore: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { messages } from '@/i18n'
import { getPendingActions, runAsyncAction } from '@/lib/async-action'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  disposeSettingsStorage,
  setSettingsStorageNotifier,
  storageIdentityKey,
  useSettingsStorageStore,
} from '@/lib/settings-storage'
import {
  RAW_OBJECT_STORE_CAPABILITY_CODE,
  STORAGE_KEY,
  buildSecretPatch,
  clearSavedSecretDrafts,
  createRawObjectStoreForm,
  formFromRawObjectStore,
  isRawObjectStoreCapabilityUnavailable,
  mergeRawObjectStoreResponse,
  normalizeStoragePrefix,
  rawObjectStoreDirty,
  rawObjectStoreFormToRequest,
  storageBucketMissing,
  storageRefreshBlocked,
} from '@/lib/settings-storage-model'
import { SettingsPage } from '@/pages/SettingsPage'
import { SettingsStorageTab } from '@/components/settings/SettingsStorageTab'

const copy = messages['en-US']

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}

const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'fixture-operator',
  user_id: 2,
}

const stored: RawObjectStoreSettingsResponse = {
  backend: 'local',
  has_s3_access_key: true,
  has_s3_secret_key: true,
  local_dir: '/var/lib/prompt-ferry/raw',
  s3_allow_http: false,
  s3_bucket: 'prompt-ferry-raw',
  s3_endpoint: 'https://s3.example.internal',
  s3_path_style: false,
  s3_prefix: 'fixture',
  s3_region: 'eu-central-1',
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, reject, resolve }
}

function queryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function store() {
  return useSettingsStorageStore.getState()
}

async function flushMicrotasks(times = 8) {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

async function loadStorage(client = queryClient()) {
  store().initializeIdentity(admin)
  await act(async () => {
    await store().refresh(client)
  })
  return client
}

function renderPage(path = '/settings/storage') {
  return render(
    <QueryClientProvider client={queryClient()}>
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={[path]}>
            <Routes>
              <Route element={<AppShell />}>
                <Route path="/settings/:tab" element={<SettingsPage />} />
              </Route>
            </Routes>
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
}

function capabilityError() {
  return {
    error: {
      code: RAW_OBJECT_STORE_CAPABILITY_CODE,
      message: 'raw object store configuration is not available on SQLite',
    },
  }
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  api.getRawObjectStore.mockReset().mockResolvedValue(stored)
  api.setRawObjectStore.mockReset()
  useSettingsStorageStore.getState().resetIdentity(null)
  setSettingsStorageNotifier(null)
  useSessionStore.setState({
    bootstrapped: true,
    bootstrapping: false,
    error: '',
    me: admin,
  })
  Element.prototype.scrollIntoView = vi.fn()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: class {
        readonly disconnect = vi.fn()
        readonly observe = vi.fn()
        readonly unobserve = vi.fn()
        constructor(_callback: ResizeObserverCallback) {}
      },
    })
  }
})

afterEach(() => {
  cleanup()
  disposeSettingsStorage()
  useSettingsStorageStore.getState().resetIdentity(null)
})

describe('storage settings pure model', () => {
  it('starts from the server defaults', () => {
    expect(createRawObjectStoreForm()).toEqual({
      backend: 'local',
      local_dir: '',
      s3_endpoint: '',
      s3_bucket: '',
      s3_region: 'auto',
      s3_prefix: 'prompt-ferry/raw',
      s3_allow_http: false,
      s3_path_style: true,
      s3_access_key_draft: '',
      s3_secret_key_draft: '',
    })
  })

  it('normalizes the prefix the way the server does', () => {
    expect(normalizeStoragePrefix('  /fixture/raw/  ')).toBe('fixture/raw')
    expect(normalizeStoragePrefix('')).toBe('prompt-ferry/raw')
    expect(normalizeStoragePrefix('///')).toBe('prompt-ferry/raw')
  })

  it('never builds a clear patch: blank keeps, non-blank replaces', () => {
    expect(buildSecretPatch('')).toEqual({ mode: 'keep' })
    expect(buildSecretPatch('   ')).toEqual({ mode: 'keep' })
    expect(buildSecretPatch(' synthetic-key ')).toEqual({
      mode: 'replace',
      value: 'synthetic-key',
    })
    const body = rawObjectStoreFormToRequest({
      ...createRawObjectStoreForm(),
      s3_region: '   ',
    })
    expect(body.s3_region).toBe('auto')
    expect(body.s3_path_style).toBe(true)
    expect(body.s3_access_key).toEqual({ mode: 'keep' })
    expect(body.s3_secret_key).toEqual({ mode: 'keep' })
  })

  it('projects a stored response to the form with empty secret drafts', () => {
    expect(formFromRawObjectStore(stored)).toEqual({
      backend: 'local',
      local_dir: '/var/lib/prompt-ferry/raw',
      s3_endpoint: 'https://s3.example.internal',
      s3_bucket: 'prompt-ferry-raw',
      s3_region: 'eu-central-1',
      s3_prefix: 'fixture',
      s3_allow_http: false,
      s3_path_style: false,
      s3_access_key_draft: '',
      s3_secret_key_draft: '',
    })
    expect(
      formFromRawObjectStore({ ...stored, s3_path_style: undefined })
        .s3_path_style,
    ).toBe(true)
  })

  it('merges only the plain fields the operator did not change', () => {
    const submitted = formFromRawObjectStore(stored)
    const current = { ...submitted, s3_bucket: 'newer-bucket' }
    const merged = mergeRawObjectStoreResponse(current, submitted, {
      ...stored,
      s3_bucket: 'server-bucket',
      s3_region: 'us-east-1',
    })
    expect(merged.s3_bucket).toBe('newer-bucket')
    expect(merged.s3_region).toBe('us-east-1')
  })

  it('clears only the secret drafts the save consumed', () => {
    const submitted = {
      ...createRawObjectStoreForm(),
      s3_access_key_draft: 'submitted-key',
      s3_secret_key_draft: 'submitted-secret',
    }
    const current = {
      ...submitted,
      s3_secret_key_draft: 'newer-secret',
    }
    const cleared = clearSavedSecretDrafts(current, submitted)
    expect(cleared.s3_access_key_draft).toBe('')
    expect(cleared.s3_secret_key_draft).toBe('newer-secret')
  })

  it('treats any secret draft as dirty and blocks a refresh accordingly', () => {
    const baseline = createRawObjectStoreForm()
    expect(rawObjectStoreDirty(baseline, baseline)).toBe(false)
    expect(
      rawObjectStoreDirty({ ...baseline, s3_secret_key_draft: 'x' }, baseline),
    ).toBe(true)

    const base = {
      pendingActions: {},
      dirty: false,
      failed: false,
      loading: false,
      loaded: true,
    }
    expect(storageRefreshBlocked(base)).toBe(false)
    expect(storageRefreshBlocked({ ...base, loaded: false })).toBe(true)
    expect(storageRefreshBlocked({ ...base, loading: true })).toBe(true)
    expect(storageRefreshBlocked({ ...base, dirty: true })).toBe(true)
    expect(storageRefreshBlocked({ ...base, failed: true })).toBe(true)
    expect(
      storageRefreshBlocked({
        ...base,
        pendingActions: { [STORAGE_KEY]: 'storage' },
      }),
    ).toBe(true)
  })

  it('needs a bucket for the s3 backend and reads the structured capability code', () => {
    expect(
      storageBucketMissing({ ...createRawObjectStoreForm(), backend: 's3' }),
    ).toBe(true)
    expect(
      storageBucketMissing({
        ...createRawObjectStoreForm(),
        backend: 's3',
        s3_bucket: ' bucket ',
      }),
    ).toBe(false)
    expect(
      storageBucketMissing({ ...createRawObjectStoreForm(), backend: 'local' }),
    ).toBe(false)

    expect(isRawObjectStoreCapabilityUnavailable(capabilityError())).toBe(true)
    expect(
      isRawObjectStoreCapabilityUnavailable(
        new Error('sqlite_raw_object_store_unavailable'),
      ),
    ).toBe(false)
    expect(
      isRawObjectStoreCapabilityUnavailable({
        error: { code: 'bad_request', message: 'x' },
      }),
    ).toBe(false)
  })
})

describe('storage settings store', () => {
  it('reads once and writes nothing', async () => {
    await loadStorage()
    expect(api.getRawObjectStore).toHaveBeenCalledTimes(1)
    expect(api.setRawObjectStore).not.toHaveBeenCalled()
    expect(store().loaded).toBe(true)
    expect(store().form.s3_bucket).toBe('prompt-ferry-raw')
    expect(store().presence).toEqual({
      has_s3_access_key: true,
      has_s3_secret_key: true,
    })
    expect(store().form.s3_access_key_draft).toBe('')
  })

  it('refuses to save before a successful read', async () => {
    store().initializeIdentity(admin)
    expect(await store().save()).toEqual({ executed: false })
    expect(api.setRawObjectStore).not.toHaveBeenCalled()
  })

  it('saves the full body with keep patches and confirms once', async () => {
    await loadStorage()
    const notifySaved = vi.fn()
    setSettingsStorageNotifier({
      notifyApiError: vi.fn(),
      notifyBucketRequired: vi.fn(),
      notifySaved,
    })
    api.setRawObjectStore.mockResolvedValueOnce(stored)

    store().setField('local_dir', '/srv/raw')
    await act(async () => {
      await store().save()
    })

    expect(api.setRawObjectStore).toHaveBeenCalledTimes(1)
    const body = api.setRawObjectStore.mock.calls[0]?.[0]?.body
    expect(body).toEqual({
      backend: 'local',
      local_dir: '/srv/raw',
      s3_endpoint: 'https://s3.example.internal',
      s3_bucket: 'prompt-ferry-raw',
      s3_region: 'eu-central-1',
      s3_prefix: 'fixture',
      s3_allow_http: false,
      s3_path_style: false,
      s3_access_key: { mode: 'keep' },
      s3_secret_key: { mode: 'keep' },
    })
    expect(notifySaved).toHaveBeenCalledTimes(1)
  })

  it('rejects an s3 save without a bucket locally, with one localized error', async () => {
    await loadStorage()
    const notifyBucketRequired = vi.fn()
    setSettingsStorageNotifier({
      notifyApiError: vi.fn(),
      notifyBucketRequired,
      notifySaved: vi.fn(),
    })

    store().setField('backend', 's3')
    store().setField('s3_bucket', '   ')
    await act(async () => {
      await store().save()
    })

    expect(api.setRawObjectStore).not.toHaveBeenCalled()
    expect(notifyBucketRequired).toHaveBeenCalledTimes(1)
    expect(Object.hasOwn(getPendingActions(), STORAGE_KEY)).toBe(false)
    expect(store().saveFailed).toBe(true)
    expect(store().form.s3_bucket).toBe('   ')
  })

  it('sends a replace patch for a typed secret and keeps an untouched one', async () => {
    await loadStorage()
    api.setRawObjectStore.mockResolvedValueOnce({
      ...stored,
      has_s3_access_key: true,
      has_s3_secret_key: true,
    })

    store().setField('s3_access_key_draft', '  synthetic-access  ')
    await act(async () => {
      await store().save()
    })
    const body = api.setRawObjectStore.mock.calls[0]?.[0]?.body
    expect(body.s3_access_key).toEqual({
      mode: 'replace',
      value: 'synthetic-access',
    })
    expect(body.s3_secret_key).toEqual({ mode: 'keep' })
    expect(store().form.s3_access_key_draft).toBe('')
    expect(store().presence.has_s3_access_key).toBe(true)
  })

  it('keeps a secret typed during the write and stays dirty', async () => {
    await loadStorage()
    const write = deferred<RawObjectStoreSettingsResponse>()
    api.setRawObjectStore.mockReturnValueOnce(write.promise)

    store().setField('s3_secret_key_draft', 'submitted-secret')
    const pending = store().save()
    await flushMicrotasks(2)
    store().setField('s3_secret_key_draft', 'newer-secret')

    write.resolve({ ...stored, has_s3_secret_key: true })
    await act(async () => {
      await pending
    })

    expect(store().form.s3_secret_key_draft).toBe('newer-secret')
    expect(rawObjectStoreDirty(store().form, store().baseline)).toBe(true)
  })

  it('reports one error, keeps the draft and retries through Save', async () => {
    await loadStorage()
    const notifyApiError = vi.fn()
    setSettingsStorageNotifier({
      notifyApiError,
      notifyBucketRequired: vi.fn(),
      notifySaved: vi.fn(),
    })
    api.setRawObjectStore
      .mockRejectedValueOnce(new Error('503 service unavailable'))
      .mockResolvedValueOnce({ ...stored, s3_bucket: 'retry-bucket' })

    store().setField('s3_bucket', 'retry-bucket')
    await act(async () => {
      await store().save()
    })
    expect(notifyApiError).toHaveBeenCalledTimes(1)
    expect(store().saveFailed).toBe(true)
    expect(store().form.s3_bucket).toBe('retry-bucket')

    await act(async () => {
      await store().save()
    })
    expect(api.setRawObjectStore).toHaveBeenCalledTimes(2)
    expect(store().saveFailed).toBe(false)
  })

  it('sends one request and one confirmation for a double save', async () => {
    await loadStorage()
    const notifySaved = vi.fn()
    setSettingsStorageNotifier({
      notifyApiError: vi.fn(),
      notifyBucketRequired: vi.fn(),
      notifySaved,
    })
    const write = deferred<RawObjectStoreSettingsResponse>()
    api.setRawObjectStore.mockReturnValueOnce(write.promise)

    const first = store().save()
    await flushMicrotasks(2)
    expect(await store().save()).toEqual({ executed: false })
    write.resolve(stored)
    await act(async () => {
      await first
    })

    expect(api.setRawObjectStore).toHaveBeenCalledTimes(1)
    expect(notifySaved).toHaveBeenCalledTimes(1)
  })

  it('does not block an unrelated settings key', async () => {
    await loadStorage()
    api.setRawObjectStore.mockResolvedValueOnce(stored)
    const release: Array<() => void> = []
    const held = runAsyncAction(
      'setting:llmReview',
      () => new Promise<void>((resolve) => release.push(resolve)),
      'settings',
    )
    await flushMicrotasks(1)

    await act(async () => {
      await store().save()
    })
    expect(api.setRawObjectStore).toHaveBeenCalledTimes(1)
    release.forEach((resolve) => resolve())
    await held
  })

  it('reports the capability state explicitly and retries the read', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    api.getRawObjectStore
      .mockRejectedValueOnce(capabilityError())
      .mockResolvedValueOnce(stored)

    await act(async () => {
      await store().refresh(client)
    })
    expect(store().unavailable).toBe(true)
    expect(store().unavailableMessage).toBe(
      `${RAW_OBJECT_STORE_CAPABILITY_CODE}: raw object store configuration is not available on SQLite`,
    )
    expect(store().loaded).toBe(false)
    expect(await store().save()).toEqual({ executed: false })
    expect(api.setRawObjectStore).not.toHaveBeenCalled()

    await act(async () => {
      await store().refresh(client)
    })
    expect(store().unavailable).toBe(false)
    expect(store().loaded).toBe(true)
  })

  it('treats a generic read failure as a retryable failure, not a capability', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    api.getRawObjectStore.mockRejectedValueOnce(new Error('500 internal'))

    await act(async () => {
      await store().refresh(client)
    })
    expect(store().unavailable).toBe(false)
    expect(store().loadError).toBe(true)
  })

  it('commits only the latest read, drops a pre-edit read and stale identity', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const resolvers: Array<(value: RawObjectStoreSettingsResponse) => void> = []
    api.getRawObjectStore.mockImplementation(
      () =>
        new Promise<RawObjectStoreSettingsResponse>((resolve) => {
          resolvers.push(resolve)
        }),
    )

    void store().refresh(client)
    void store().refresh(client)
    await flushMicrotasks(4)
    const current = resolvers.at(-1) as (
      value: RawObjectStoreSettingsResponse,
    ) => void
    current({ ...stored, s3_bucket: 'latest' })
    await flushMicrotasks(8)
    expect(store().form.s3_bucket).toBe('latest')
    for (const stale of resolvers.slice(0, -1)) {
      stale({ ...stored, s3_bucket: 'stale' })
    }
    await flushMicrotasks(8)
    expect(store().form.s3_bucket).toBe('latest')

    const slow = deferred<RawObjectStoreSettingsResponse>()
    api.getRawObjectStore.mockReturnValueOnce(slow.promise)
    void store().refresh(client)
    await flushMicrotasks(2)
    store().setField('s3_bucket', 'edited')
    expect(store().loading).toBe(false)
    slow.resolve({ ...stored, s3_bucket: 'late' })
    await flushMicrotasks(8)
    expect(store().form.s3_bucket).toBe('edited')

    const identityRead = deferred<RawObjectStoreSettingsResponse>()
    api.getRawObjectStore.mockReturnValueOnce(identityRead.promise)
    const read = store().refresh(client)
    store().resetIdentity(operator)
    identityRead.resolve(stored)
    await act(async () => {
      await read
    })
    expect(store().identityKey).toBe(storageIdentityKey(operator))
    expect(store().loaded).toBe(false)
    expect(store().form.s3_access_key_draft).toBe('')
  })

  it('refuses a refresh while dirty and never refetches on rerender', async () => {
    const client = await loadStorage()
    store().setField('local_dir', '/dirty')
    await act(async () => {
      await store().refresh(client)
    })
    expect(api.getRawObjectStore).toHaveBeenCalledTimes(1)

    store().resetIdentity(admin)
    const fresh = await loadStorage(queryClient())
    await act(async () => {
      await store().ensureLoaded(fresh)
    })
    expect(api.getRawObjectStore).toHaveBeenCalledTimes(2)
  })

  it('clears the secret drafts on unmount', async () => {
    const client = await loadStorage()
    store().setField('s3_access_key_draft', 'draft-secret')
    disposeSettingsStorage()
    expect(store().form.s3_access_key_draft).toBe('')
    expect(store().form.s3_secret_key_draft).toBe('')
    expect(store().loaded).toBe(false)

    await act(async () => {
      await store().ensureLoaded(client)
    })
    expect(api.getRawObjectStore).toHaveBeenCalledTimes(2)
  })
})

describe('storage settings tab', () => {
  it('renders the local branch with the backend select and no save-on-mount', async () => {
    renderPage()
    await flushMicrotasks()

    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-STORAGE',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('storage')
    expect(
      screen.getByRole('heading', { name: copy.settingsStorage }),
    ).toBeTruthy()
    expect(
      (document.getElementById('settings-storage-backend') as HTMLElement)
        .textContent,
    ).toContain(copy.storageBackendLocal)
    expect(document.getElementById('settings-storage-local-dir')).not.toBeNull()
    expect(screen.getByText(copy.storageLocalHint)).toBeTruthy()
    expect(api.setRawObjectStore).not.toHaveBeenCalled()
  })

  it('switches to the S3 branch with credentials, presence badges and hints', async () => {
    renderPage()
    await flushMicrotasks()

    await act(async () => {
      store().setField('backend', 's3')
    })

    for (const id of [
      'settings-storage-s3-endpoint',
      'settings-storage-s3-bucket',
      'settings-storage-s3-region',
      'settings-storage-s3-prefix',
      'settings-storage-s3-allow-http',
      'settings-storage-s3-path-style',
      'settings-storage-s3-access-key',
      'settings-storage-s3-secret-key',
    ]) {
      expect(document.getElementById(id), id).not.toBeNull()
    }
    expect(
      document
        .getElementById('settings-storage-s3-access-key')
        ?.getAttribute('type'),
    ).toBe('password')
    expect(
      document
        .getElementById('settings-storage-s3-secret-key')
        ?.getAttribute('type'),
    ).toBe('password')
    expect(screen.getAllByText(copy.storageSecretPresent)).toHaveLength(2)
    expect(screen.getByText(copy.storageSecretHint)).toBeTruthy()
    expect(screen.getByText(copy.storageS3AllowHttpHelp)).toBeTruthy()
    expect(screen.getByText(copy.storageS3PathStyleHelp)).toBeTruthy()
    expect(document.getElementById('settings-storage-local-dir')).toBeNull()
  })

  it('shows only the hint for the disabled branch', async () => {
    renderPage()
    await flushMicrotasks()

    await act(async () => {
      store().setField('backend', 'disabled')
    })
    expect(screen.getByText(copy.storageDisabledHint)).toBeTruthy()
    expect(document.getElementById('settings-storage-s3-bucket')).toBeNull()
    expect(document.getElementById('settings-storage-local-dir')).toBeNull()
  })

  it('returns the localized bucket error with no request', async () => {
    renderPage()
    await flushMicrotasks()

    await act(async () => {
      store().setField('backend', 's3')
      store().setField('s3_bucket', '')
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById('settings-storage-save') as HTMLButtonElement,
      )
    })
    await flushMicrotasks()

    expect(api.setRawObjectStore).not.toHaveBeenCalled()
    await waitFor(() =>
      expect(screen.getByText(copy.storageBucketRequired)).toBeTruthy(),
    )
  })

  it('shows the capability warning with its retry, and never the form', async () => {
    api.getRawObjectStore
      .mockRejectedValueOnce(capabilityError())
      .mockResolvedValueOnce(stored)
    renderPage()
    await flushMicrotasks()

    expect(screen.getByText(copy.storageUnavailable)).toBeTruthy()
    expect(screen.getByText(/sqlite_raw_object_store_unavailable/)).toBeTruthy()
    expect(document.getElementById('settings-storage-s3-bucket')).toBeNull()
    expect(
      (document.getElementById('settings-storage-save') as HTMLButtonElement)
        .disabled,
    ).toBe(true)

    fireEvent.click(
      document.getElementById('settings-storage-retry') as HTMLButtonElement,
    )
    await flushMicrotasks()
    expect(api.getRawObjectStore).toHaveBeenCalledTimes(2)
    await waitFor(() =>
      expect(
        document.getElementById('settings-storage-local-dir'),
      ).not.toBeNull(),
    )
  })

  it('offers no secret-clear control and never renders a secret value', async () => {
    renderPage()
    await flushMicrotasks()
    await act(async () => {
      store().setField('backend', 's3')
      store().setField('s3_access_key_draft', 'typed-access-key')
    })

    expect(
      document.body.textContent?.includes('fixture-synthetic-s3-access-key'),
    ).toBe(false)
    expect(
      document.body.textContent?.includes('fixture-synthetic-s3-secret'),
    ).toBe(false)
    for (const node of document.querySelectorAll('[title]')) {
      expect(node.getAttribute('title') ?? '').not.toContain('synthetic')
    }
    const storedValues = Object.keys(localStorage).map((key) =>
      String(localStorage.getItem(key)),
    )
    expect(storedValues.join('\n')).not.toContain('typed-access-key')
  })

  it('blocks the scoped refresh while the draft is dirty', async () => {
    renderPage()
    await flushMicrotasks()

    const refreshButton = document.getElementById(
      'settings-storage-refresh',
    ) as HTMLButtonElement
    expect(refreshButton.disabled).toBe(false)

    await act(async () => {
      store().setField('local_dir', '/dirty-dir')
    })
    expect(refreshButton.disabled).toBe(true)
  })

  it('keeps an ordinary user out of the storage route', async () => {
    useSessionStore.setState({ me: operator })
    renderPage()
    await flushMicrotasks()
    expect(
      document.querySelector('[data-foundation-route-id="R-SETTINGS-STORAGE"]'),
    ).toBeNull()
    expect(api.getRawObjectStore).not.toHaveBeenCalled()
  })

  it('renders the tab card without issuing its own read', async () => {
    render(
      <QueryClientProvider client={queryClient()}>
        <LocaleProvider>
          <NotifierProvider>
            <SettingsStorageTab />
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await flushMicrotasks()
    expect(api.getRawObjectStore).not.toHaveBeenCalled()
    expect(document.getElementById('settings-storage-backend')).not.toBeNull()
  })
})
