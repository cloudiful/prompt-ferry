/**
 * Key grammar for the guarded settings writes.
 *
 * A settings page is several forms on one screen, so the gate is per resource
 * rather than per page: a double-clicked Save must not send the resource's PUT
 * twice, and it must not stop an unrelated resource from being saved while the
 * first is still in flight.
 *
 * The key names a resource, not a form. Most of the page's forms write their own
 * thing and get their own key, but the two retention forms are one stored value
 * and share one — see `SETTING_FORM_SECTIONS`.
 */

import { recordActionKey } from '@/stores/record-action-types'
import { capturePayload } from '@/stores/record-action-types'
import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  RawObjectStoreSettingsRequest,
  RawObjectStoreSettingsResponse,
} from '@/generated/admin-api'

/** The namespace every gated settings key lives under. */
export const SETTING_KEY_PREFIX = 'setting'

/** One gated settings resource, as the surfaces name it. */
export type SettingSection =
  | 'rawRetention'
  | 'streamDeltaBatching'
  | 'relayIpWhitelist'
  | 'modelRouteWhitelist'
  | 'llmReview'
  | 'cacheAlert'
  | 'storage'

/** A settings form, as the page offers it. Forms may share a section. */
export type SettingForm =
  | 'requestContentLogging'
  | 'usageRetention'
  | 'streamDeltaBatching'
  | 'relayIpWhitelist'
  | 'modelRouteWhitelist'
  | 'llmReview'

/**
 * Which resource each form writes, and why the retention pair shares one.
 *
 * `set_request_content_logging` does not only write the logging mode: it reads
 * the whole usage-retention record, overwrites the retention field on it, and
 * writes the record back (`src/db/settings.rs`). So a logging save carries every
 * retention field through a read-modify-write of a single JSON blob, and a
 * retention save landing inside that window is reverted field for field, not
 * just for the field the operator was editing.
 *
 * One key is therefore the honest model rather than a conservative one: the two
 * forms are one stored value, so only one of them may be in flight. The form that
 * is held off stays dirty and retries once the key is released.
 */
export const SETTING_FORM_SECTIONS: Readonly<
  Record<SettingForm, SettingSection>
> = {
  requestContentLogging: 'rawRetention',
  usageRetention: 'rawRetention',
  streamDeltaBatching: 'streamDeltaBatching',
  relayIpWhitelist: 'relayIpWhitelist',
  modelRouteWhitelist: 'modelRouteWhitelist',
  llmReview: 'llmReview',
}

export function settingKey(section: SettingSection): string {
  return recordActionKey(SETTING_KEY_PREFIX, section)
}

/** The key a form's save is claimed under. Two forms may resolve to one key. */
export function settingFormKey(form: SettingForm): string {
  return settingKey(SETTING_FORM_SECTIONS[form])
}

export type SettingsResourceDeps = {
  guard: AsyncActionGuard
  /** The existing cache-alert save, gated here rather than at its own call site. */
  saveCacheAlert: () => Promise<void>
  /** The existing storage save, which answers the settings the server accepted. */
  saveRawObjectStore: (
    payload: RawObjectStoreSettingsRequest,
  ) => Promise<RawObjectStoreSettingsResponse>
}

/**
 * The two resource saves that carry an explicit payload or response.
 *
 * Their bodies belong to their own modules, so they are gated from here rather
 * than rewritten: the claim is the part that was missing, not the request.
 */
export function createSettingsResourceActions(deps: SettingsResourceDeps) {
  const { guard, saveCacheAlert, saveRawObjectStore } = deps

  function guardedCacheAlertSave(): Promise<AsyncActionOutcome<void>> {
    return guard.run(settingKey('cacheAlert'), saveCacheAlert, 'settings')
  }

  function guardedStorageSave(
    payload: RawObjectStoreSettingsRequest,
  ): Promise<AsyncActionOutcome<RawObjectStoreSettingsResponse | undefined>> {
    // Snapshot the payload before the first await, so a form edit made while the
    // request is in flight cannot ride along with the one that was submitted.
    const body = capturePayload(payload)
    return guard.run(
      settingKey('storage'),
      async () => saveRawObjectStore(body),
      'settings',
    )
  }

  return {
    saveCacheAlert: guardedCacheAlertSave,
    saveRawObjectStore: guardedStorageSave,
  }
}

export type SettingsResourceActions = ReturnType<
  typeof createSettingsResourceActions
>
