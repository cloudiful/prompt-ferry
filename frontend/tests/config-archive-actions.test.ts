import { expect, test } from 'bun:test'
import {
  applyConfigImportArchive,
  deferred,
  exportConfigArchive,
  freshArchiveStore,
  previewConfigImportArchive,
} from './config-archive-actions.fixtures'

const archiveFile = { name: 'backup.pfce', size: 3 } as File

async function selectAndPreview(
  store: ReturnType<typeof freshArchiveStore>,
): Promise<void> {
  store.importPassphrase.value = 'archive-passphrase'
  await store.selectImportFile(archiveFile)
  const preview = await store.previewImport()
  expect(preview).toMatchObject({ executed: true })
}

test('export is claimed once and an older completion preserves a new draft', async () => {
  const store = freshArchiveStore()
  const gate = deferred<void>()
  exportConfigArchive.mockImplementation(async (passphrase: string) => {
    await gate.promise
    return {
      backendKind: 'sqlite',
      bytes: new Uint8Array([65]).buffer,
      formatVersion: 1,
      payloadFingerprint: passphrase,
    }
  })
  store.exportPassphrase.value = 'first-passphrase'

  const exporting = store.exportArchive()
  expect(store.exportPending.value).toBe(true)
  store.exportPassphrase.value = 'new-passphrase'
  await expect(store.exportArchive()).resolves.toEqual({ executed: false })
  expect(exportConfigArchive).toHaveBeenCalledTimes(1)

  gate.resolve()
  await expect(exporting).resolves.toMatchObject({ executed: true })
  expect(exportConfigArchive).toHaveBeenCalledWith('first-passphrase')
  expect(store.exportPassphrase.value).toBe('new-passphrase')
  expect(store.exportPending.value).toBe(false)
})

test('preview and apply share a wizard gate and apply requires confirmation state', async () => {
  const store = freshArchiveStore()
  await store.selectImportFile(archiveFile)
  store.importPassphrase.value = 'first-passphrase'

  const previewGate = deferred<void>()
  previewConfigImportArchive.mockImplementationOnce(async () => {
    await previewGate.promise
    return {
      backend_kind: 'sqlite',
      domains: [],
      exported_at: '2026-10-12T00:00:00Z',
      format_version: 1,
      payload_fingerprint: 'preview-fingerprint',
      warnings: [],
    }
  })
  const previewing = store.previewImport()
  expect(store.importPending.value).toBe(true)
  store.importPassphrase.value = 'new-passphrase'
  await expect(store.previewImport()).resolves.toEqual({ executed: false })
  await expect(store.applyImport()).resolves.toEqual({ executed: false })
  store.cancelImport()
  expect(store.importFile.value).toEqual({ name: 'backup.pfce', size: 3 })

  previewGate.resolve()
  await expect(previewing).resolves.toMatchObject({ executed: true })
  expect(previewConfigImportArchive).toHaveBeenCalledTimes(1)
  expect(previewConfigImportArchive).toHaveBeenCalledWith(
    expect.any(String),
    'first-passphrase',
  )
  // A completion for the previous wizard input cannot replace the new draft.
  expect(store.importPreview.value).toBeNull()
  expect(store.importPassphrase.value).toBe('new-passphrase')
  expect(store.importPending.value).toBe(false)
})

test('failed apply keeps its confirmed wizard for a deliberate retry', async () => {
  const store = freshArchiveStore()
  await selectAndPreview(store)
  applyConfigImportArchive.mockImplementationOnce(async () => {
    throw new Error('archive apply rejected')
  })

  await expect(store.applyImport()).rejects.toThrow('archive apply rejected')
  expect(store.importPending.value).toBe(false)
  expect(store.importFile.value).toEqual({ name: 'backup.pfce', size: 3 })
  expect(store.importPreview.value?.payload_fingerprint).toBe(
    'preview-fingerprint',
  )
  expect(store.importPassphrase.value).toBe('archive-passphrase')

  await expect(store.applyImport()).resolves.toMatchObject({ executed: true })
  expect(applyConfigImportArchive).toHaveBeenCalledTimes(2)
  expect(store.importApplied.value?.payload_fingerprint).toBe(
    'applied-fingerprint',
  )
  expect(store.importFile.value).toBeNull()
  expect(store.importPassphrase.value).toBe('')
  expect(store.importPreview.value).toBeNull()
})

test('only one held apply runs and its follow-up audit remains inside the gate', async () => {
  const store = freshArchiveStore()
  await selectAndPreview(store)
  const gate = deferred<void>()
  applyConfigImportArchive.mockImplementation(async () => {
    await gate.promise
    return {
      backend_kind: 'sqlite',
      domains: [],
      format_version: 1,
      payload_fingerprint: 'applied-fingerprint',
    }
  })

  const applying = store.applyImport()
  expect(store.importPending.value).toBe(true)
  await expect(store.applyImport()).resolves.toEqual({ executed: false })
  store.cancelImport()
  expect(store.importFile.value).not.toBeNull()
  expect(applyConfigImportArchive).toHaveBeenCalledTimes(1)

  gate.resolve()
  await expect(applying).resolves.toMatchObject({ executed: true })
  expect(store.importPending.value).toBe(false)
  expect(store.importApplied.value?.payload_fingerprint).toBe(
    'applied-fingerprint',
  )
  expect(store.importFile.value).toBeNull()
})
