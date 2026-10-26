/**
 * Key grammar for the guarded settings writes.
 *
 * A settings page is several forms on one screen, so the gate is per resource
 * rather than per page: a double-clicked save must not send the resource twice,
 * and it must not stop an unrelated resource from being saved while the first
 * is still in flight.
 *
 * The key names a resource, not a form. Most forms write their own thing and get
 * their own key, but the two retention forms are one stored value and share one
 * — see `SETTING_FORM_SECTIONS`.
 */

import { recordActionKey } from '@/stores/record-action-types'

/** The namespace every gated settings key lives under. */
export const SETTING_KEY_PREFIX = 'setting'

/** One gated settings resource, as the surfaces name it. */
export type SettingSection =
  | 'rawRetention'
  | 'streamDeltaBatching'
  | 'modelRouteWhitelist'
  | 'relayIpWhitelist'
  | 'llmReview'
  | 'storage'
  | 'cacheAlert'

/** A settings form, as the requests tab offers it. Forms may share a section. */
export type SettingForm =
  | 'requestContentLogging'
  | 'usageRetention'
  | 'streamDeltaBatching'
  | 'modelRouteWhitelist'

/**
 * Which resource each form writes, and why the retention pair shares one.
 *
 * `set_request_content_logging` does not only write the logging mode: it reads
 * the whole usage-retention record, overwrites the raw-retention field on it,
 * and writes the record back. So a logging save carries every retention field
 * through a read-modify-write of a single stored value, and a retention save
 * landing inside that window is reverted field for field.
 *
 * One key is therefore the honest model rather than a conservative one: the two
 * forms are one stored value, so only one of them may be in flight. The form
 * that is held off stays dirty and retries once the key is released.
 */
export const SETTING_FORM_SECTIONS: Readonly<
  Record<SettingForm, SettingSection>
> = {
  requestContentLogging: 'rawRetention',
  usageRetention: 'rawRetention',
  streamDeltaBatching: 'streamDeltaBatching',
  modelRouteWhitelist: 'modelRouteWhitelist',
}

/** Every form of the requests tab, in a stable order. */
export const SETTING_FORM_ORDER: ReadonlyArray<SettingForm> = [
  'requestContentLogging',
  'usageRetention',
  'streamDeltaBatching',
  'modelRouteWhitelist',
]

export function settingKey(section: SettingSection): string {
  return recordActionKey(SETTING_KEY_PREFIX, section)
}

/** The key a form's save is claimed under. Two forms may resolve to one key. */
export function settingFormKey(form: SettingForm): string {
  return settingKey(SETTING_FORM_SECTIONS[form])
}
