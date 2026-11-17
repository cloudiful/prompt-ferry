/**
 * Key grammar for the guarded settings writes.
 *
 * A settings page is several forms on one screen, so the gate is per resource
 * rather than per page: a double-clicked save must not send the resource twice,
 * and it must not stop an unrelated resource from being saved while the first
 * is still in flight.
 *
 * The key names a resource, not a form.
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
  | 'storage'
  | 'sharedValkey'
  | 'cacheAlert'

/** A settings form, as the requests tab offers it. Forms may share a section. */
export type SettingForm =
  'requestContentLogging' | 'streamDeltaBatching' | 'modelRouteWhitelist'

/** Which resource each form writes. */
export const SETTING_FORM_SECTIONS: Readonly<
  Record<SettingForm, SettingSection>
> = {
  requestContentLogging: 'rawRetention',
  streamDeltaBatching: 'streamDeltaBatching',
  modelRouteWhitelist: 'modelRouteWhitelist',
}

/** Every form of the requests tab, in a stable order. */
export const SETTING_FORM_ORDER: ReadonlyArray<SettingForm> = [
  'requestContentLogging',
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
