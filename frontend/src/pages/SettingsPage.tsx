import { useLocation } from 'react-router'
import { SettingsCacheAlertTab } from '@/components/settings/SettingsCacheAlertTab'
import { SettingsConfigArchiveTab } from '@/components/settings/SettingsConfigArchiveTab'
import { SettingsGeneralTab } from '@/components/settings/SettingsGeneralTab'
import { SettingsNetworkTab } from '@/components/settings/SettingsNetworkTab'
import { SettingsRequestsTab } from '@/components/settings/SettingsRequestsTab'
import { SettingsReviewTab } from '@/components/settings/SettingsReviewTab'
import { SettingsStorageTab } from '@/components/settings/SettingsStorageTab'
import { useLocale } from '@/lib/i18n'
import { usePageTitle } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import { resolveSettingsTab } from '@/models/settings'

function currentSettingsTab(pathname: string): string | null {
  const [, section, tab] = pathname.split('/')
  return section === 'settings' ? (tab ?? null) : null
}

/**
 * The mounted settings route.
 *
 * The general tab and all six admin tabs (requests, network, review, storage,
 * cache alert, config archive) are ported; a path that names any other tab
 * renders nothing rather than a stub.
 * The resolver is applied on every path, so an unauthenticated or non-admin
 * resolution can never open an admin tab here.
 */
export function SettingsPage() {
  const { t } = useLocale()
  const location = useLocation()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const requested = currentSettingsTab(location.pathname)
  const tab = resolveSettingsTab(requested, isAdmin)

  usePageTitle(
    tab === 'requests'
      ? 'settingsUsage'
      : tab === 'network'
        ? 'settingsNetwork'
        : tab === 'review'
          ? 'settingsReview'
          : tab === 'storage'
            ? 'settingsStorage'
            : tab === 'cache-alert'
              ? 'settingsCacheAlert'
              : tab === 'config-archive'
                ? 'settingsConfigArchive'
                : 'settingsGeneral',
  )

  if (requested === 'requests' && tab === 'requests') {
    return (
      <section
        aria-label={t('settingsUsage')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-REQUESTS"
        data-settings-tab={tab}
      >
        <SettingsRequestsTab />
      </section>
    )
  }

  if (requested === 'network' && tab === 'network') {
    return (
      <section
        aria-label={t('settingsNetwork')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-NETWORK"
        data-settings-tab={tab}
      >
        <SettingsNetworkTab />
      </section>
    )
  }

  if (requested === 'review' && tab === 'review') {
    return (
      <section
        aria-label={t('settingsReview')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-REVIEW"
        data-settings-tab={tab}
      >
        <SettingsReviewTab />
      </section>
    )
  }

  if (requested === 'storage' && tab === 'storage') {
    return (
      <section
        aria-label={t('settingsStorage')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-STORAGE"
        data-settings-tab={tab}
      >
        <SettingsStorageTab />
      </section>
    )
  }

  if (requested === 'cache-alert' && tab === 'cache-alert') {
    return (
      <section
        aria-label={t('settingsCacheAlert')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-CACHE-ALERT"
        data-settings-tab={tab}
      >
        <SettingsCacheAlertTab />
      </section>
    )
  }

  if (requested === 'config-archive' && tab === 'config-archive') {
    return (
      <section
        aria-label={t('settingsConfigArchive')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-CONFIG-ARCHIVE"
        data-settings-tab={tab}
      >
        <SettingsConfigArchiveTab />
      </section>
    )
  }

  if (requested !== 'general' || tab !== 'general') return null

  return (
    <section
      aria-label={t('settingsGeneral')}
      className="grid min-w-0 gap-3"
      data-foundation-route-id="R-SETTINGS-GENERAL"
      data-settings-tab={tab}
    >
      <SettingsGeneralTab />
    </section>
  )
}
