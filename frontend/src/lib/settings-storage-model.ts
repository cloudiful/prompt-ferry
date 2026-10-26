/**
 * Pure model for the admin Storage settings tab: the raw object store.
 *
 * The stored S3 credentials are write-only from the client's point of view. The
 * server never echoes them back; the response carries only two presence flags.
 * So the form keeps the operator's drafts separately from the flags, and the
 * only patches it can build are `keep` (blank draft) and `replace` (non-blank
 * draft). There is no clear control: `clear` exists in the API contract, not on
 * this surface.
 *
 * Defaults and normalization mirror the server: an empty region becomes `auto`,
 * the prefix is trimmed and stripped of surrounding slashes with an empty value
 * falling back to `prompt-ferry/raw`, and `s3_path_style` defaults to true.
 */

import type {
  RawObjectStoreBackend,
  RawObjectStoreSecretPatch,
  RawObjectStoreSettingsRequest,
  RawObjectStoreSettingsResponse,
} from '@/generated/admin-api'
import type { PendingActions } from './async-action'
import { settingKey } from './settings-keys'

/** The form as the tab shows it, including the two write-only drafts. */
export type RawObjectStoreForm = {
  backend: RawObjectStoreBackend
  local_dir: string
  s3_endpoint: string
  s3_bucket: string
  s3_region: string
  s3_prefix: string
  s3_allow_http: boolean
  s3_path_style: boolean
  s3_access_key_draft: string
  s3_secret_key_draft: string
}

/** The stored presence of each secret, as the redacted response reports it. */
export type RawObjectStorePresence = {
  has_s3_access_key: boolean
  has_s3_secret_key: boolean
}

export type RawObjectStoreField = keyof RawObjectStoreForm

/** The editable, non-secret fields: the only leaves a response may overwrite. */
export const RAW_OBJECT_STORE_PLAIN_FIELDS: ReadonlyArray<RawObjectStoreField> =
  [
    'backend',
    'local_dir',
    's3_endpoint',
    's3_bucket',
    's3_region',
    's3_prefix',
    's3_allow_http',
    's3_path_style',
  ]

export const STORAGE_KEY = settingKey('storage')

export const DEFAULT_S3_REGION = 'auto'
export const DEFAULT_S3_PREFIX = 'prompt-ferry/raw'

export const BACKEND_OPTIONS: ReadonlyArray<{
  labelKey:
    'storageBackendLocal' | 'storageBackendS3' | 'storageBackendDisabled'
  value: RawObjectStoreBackend
}> = [
  { labelKey: 'storageBackendLocal', value: 'local' },
  { labelKey: 'storageBackendS3', value: 's3' },
  { labelKey: 'storageBackendDisabled', value: 'disabled' },
]

export function createRawObjectStoreForm(): RawObjectStoreForm {
  return {
    backend: 'local',
    local_dir: '',
    s3_endpoint: '',
    s3_bucket: '',
    s3_region: DEFAULT_S3_REGION,
    s3_prefix: DEFAULT_S3_PREFIX,
    s3_allow_http: false,
    s3_path_style: true,
    s3_access_key_draft: '',
    s3_secret_key_draft: '',
  }
}

/** The prefix exactly as the server normalizes it. */
export function normalizeStoragePrefix(prefix: string): string {
  const trimmed = prefix.trim().replace(/^\/+|\/+$/g, '')
  return trimmed === '' ? DEFAULT_S3_PREFIX : trimmed
}

/** A stored response projected onto the form, with both secret drafts empty. */
export function formFromRawObjectStore(
  response: RawObjectStoreSettingsResponse,
): RawObjectStoreForm {
  return {
    backend: response.backend,
    local_dir: response.local_dir,
    s3_endpoint: response.s3_endpoint,
    s3_bucket: response.s3_bucket,
    s3_region: response.s3_region || DEFAULT_S3_REGION,
    s3_prefix: response.s3_prefix,
    s3_allow_http: response.s3_allow_http,
    s3_path_style: response.s3_path_style ?? true,
    s3_access_key_draft: '',
    s3_secret_key_draft: '',
  }
}

export function presenceFromRawObjectStore(
  response: RawObjectStoreSettingsResponse,
): RawObjectStorePresence {
  return {
    has_s3_access_key: response.has_s3_access_key,
    has_s3_secret_key: response.has_s3_secret_key,
  }
}

/**
 * The secret patch for one draft: a blank draft keeps the stored value, a
 * non-blank one replaces it with the trimmed entry. The form never clears.
 */
export function buildSecretPatch(draft: string): RawObjectStoreSecretPatch {
  const trimmed = draft.trim()
  return trimmed === '' ? { mode: 'keep' } : { mode: 'replace', value: trimmed }
}

/** The full PATCH body, with the server's region and prefix normalization. */
export function rawObjectStoreFormToRequest(
  form: RawObjectStoreForm,
): RawObjectStoreSettingsRequest {
  return {
    backend: form.backend,
    local_dir: form.local_dir,
    s3_endpoint: form.s3_endpoint,
    s3_bucket: form.s3_bucket,
    s3_region: form.s3_region.trim() || DEFAULT_S3_REGION,
    s3_prefix: form.s3_prefix,
    s3_allow_http: form.s3_allow_http,
    s3_path_style: form.s3_path_style,
    s3_access_key: buildSecretPatch(form.s3_access_key_draft),
    s3_secret_key: buildSecretPatch(form.s3_secret_key_draft),
  }
}

/**
 * The client-side bucket guard: the s3 backend needs a bucket, and the server
 * rejects the same combination with its own 400.
 */
export function storageBucketMissing(form: RawObjectStoreForm): boolean {
  return form.backend === 's3' && form.s3_bucket.trim() === ''
}

/**
 * Apply a saved response to the plain fields the operator has not changed since
 * the submit. The secret drafts are handled by `clearSavedSecretDrafts`: a draft
 * typed during the write is a newer edit and is never erased by the response.
 */
export function mergeRawObjectStoreResponse(
  current: RawObjectStoreForm,
  submitted: RawObjectStoreForm,
  response: RawObjectStoreSettingsResponse,
): RawObjectStoreForm {
  const normalized = formFromRawObjectStore(response)
  const merged: RawObjectStoreForm = { ...current }
  for (const field of RAW_OBJECT_STORE_PLAIN_FIELDS) {
    if (Object.is(current[field], submitted[field])) {
      Object.assign(merged, { [field]: normalized[field] })
    }
  }
  return merged
}

/**
 * Clear the secret drafts a successful save consumed, and only those: a draft
 * that no longer matches the captured one is a newer edit and stays.
 */
export function clearSavedSecretDrafts(
  current: RawObjectStoreForm,
  submitted: RawObjectStoreForm,
): RawObjectStoreForm {
  return {
    ...current,
    s3_access_key_draft: Object.is(
      current.s3_access_key_draft,
      submitted.s3_access_key_draft,
    )
      ? ''
      : current.s3_access_key_draft,
    s3_secret_key_draft: Object.is(
      current.s3_secret_key_draft,
      submitted.s3_secret_key_draft,
    )
      ? ''
      : current.s3_secret_key_draft,
  }
}

export function rawObjectStoreDirty(
  current: RawObjectStoreForm,
  baseline: RawObjectStoreForm,
): boolean {
  if (
    current.s3_access_key_draft !== '' ||
    current.s3_secret_key_draft !== ''
  ) {
    return true
  }
  return RAW_OBJECT_STORE_PLAIN_FIELDS.some(
    (field) => current[field] !== baseline[field],
  )
}

/**
 * Whether a scoped refresh is safe: loaded, idle, clean and without an
 * unrecovered save failure.
 */
export function storageRefreshBlocked(input: {
  pendingActions: PendingActions
  dirty: boolean
  failed: boolean
  loading: boolean
  loaded: boolean
}): boolean {
  return (
    !input.loaded ||
    input.loading ||
    input.dirty ||
    input.failed ||
    Object.hasOwn(input.pendingActions, STORAGE_KEY)
  )
}

/**
 * Detect the structured capability-unavailable answer for the raw object store.
 *
 * The worker answers 501 with the capability code and its description when the
 * configured store backend cannot keep this setting (SQLite). The check reads
 * the structured code, never a message substring.
 */
export const RAW_OBJECT_STORE_CAPABILITY_CODE =
  'sqlite_raw_object_store_unavailable'

export function isRawObjectStoreCapabilityUnavailable(cause: unknown): boolean {
  if (!cause || typeof cause !== 'object') return false
  if (!('error' in cause)) return false
  const envelope = (cause as { error?: unknown }).error
  if (!envelope || typeof envelope !== 'object') return false
  return (
    (envelope as { code?: unknown }).code === RAW_OBJECT_STORE_CAPABILITY_CODE
  )
}
