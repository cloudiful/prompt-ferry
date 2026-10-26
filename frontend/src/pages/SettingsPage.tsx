import { useEffect } from 'react'
import { useLocation } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
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
import {
  configArchiveIdentityKey,
  useSettingsConfigArchiveStore,
} from '@/lib/config-archive'
import {
  cacheAlertIdentityKey,
  useSettingsCacheAlertStore,
} from '@/lib/settings-cache-alert'
import {
  cacheAlertDirty,
  cacheAlertRefreshBlocked,
} from '@/lib/settings-cache-alert-model'
import {
  networkIdentityKey,
  useSettingsNetworkStore,
} from '@/lib/settings-network'
import {
  relayWhitelistDirtyFields,
  relayWhitelistRefreshBlocked,
} from '@/lib/settings-network-model'
import { settingsRefreshBlocked } from '@/lib/settings-requests-model'
import {
  settingsIdentityKey,
  useSettingsRequestsStore,
} from '@/lib/settings-requests'
import {
  reviewIdentityKey,
  useSettingsReviewStore,
} from '@/lib/settings-review'
import {
  llmReviewDirtyFields,
  llmReviewRefreshBlocked,
} from '@/lib/settings-review-model'
import {
  storageIdentityKey,
  useSettingsStorageStore,
} from '@/lib/settings-storage'
import {
  rawObjectStoreDirty,
  storageRefreshBlocked,
} from '@/lib/settings-storage-model'
import { useAsyncAction } from '@/lib/use-async-action'
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
  const queryClient = useQueryClient()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const requested = currentSettingsTab(location.pathname)
  const tab = resolveSettingsTab(requested, isAdmin)

  const identityKey = settingsIdentityKey(me)
  const initializeIdentity = useSettingsRequestsStore(
    (state) => state.initializeIdentity,
  )
  const ensureLoaded = useSettingsRequestsStore((state) => state.ensureLoaded)
  const refresh = useSettingsRequestsStore((state) => state.refresh)
  const loaded = useSettingsRequestsStore((state) => state.loaded)
  const loading = useSettingsRequestsStore((state) => state.loading)
  const dirty = useSettingsRequestsStore((state) => state.dirty)
  const failed = useSettingsRequestsStore((state) => state.failed)

  const networkIdentity = networkIdentityKey(me)
  const initializeNetworkIdentity = useSettingsNetworkStore(
    (state) => state.initializeIdentity,
  )
  const ensureNetworkLoaded = useSettingsNetworkStore(
    (state) => state.ensureLoaded,
  )
  const refreshNetwork = useSettingsNetworkStore((state) => state.refresh)
  const networkForm = useSettingsNetworkStore((state) => state.form)
  const networkBaseline = useSettingsNetworkStore((state) => state.baseline)
  const networkLoaded = useSettingsNetworkStore((state) => state.loaded)
  const networkLoading = useSettingsNetworkStore((state) => state.loading)
  const networkSaveFailed = useSettingsNetworkStore((state) => state.saveFailed)
  const reviewIdentity = reviewIdentityKey(me)
  const initializeReviewIdentity = useSettingsReviewStore(
    (state) => state.initializeIdentity,
  )
  const resetReviewIdentity = useSettingsReviewStore(
    (state) => state.resetIdentity,
  )
  const ensureReviewLoaded = useSettingsReviewStore(
    (state) => state.ensureLoaded,
  )
  const refreshReview = useSettingsReviewStore((state) => state.refresh)
  const reviewForm = useSettingsReviewStore((state) => state.form)
  const reviewBaseline = useSettingsReviewStore((state) => state.baseline)
  const reviewLoaded = useSettingsReviewStore((state) => state.loaded)
  const reviewLoading = useSettingsReviewStore((state) => state.loading)
  const reviewSaveFailed = useSettingsReviewStore((state) => state.saveFailed)
  const storageIdentity = storageIdentityKey(me)
  const initializeStorageIdentity = useSettingsStorageStore(
    (state) => state.initializeIdentity,
  )
  const resetStorageIdentity = useSettingsStorageStore(
    (state) => state.resetIdentity,
  )
  const ensureStorageLoaded = useSettingsStorageStore(
    (state) => state.ensureLoaded,
  )
  const refreshStorage = useSettingsStorageStore((state) => state.refresh)
  const storageForm = useSettingsStorageStore((state) => state.form)
  const storageBaseline = useSettingsStorageStore((state) => state.baseline)
  const storageLoaded = useSettingsStorageStore((state) => state.loaded)
  const storageLoading = useSettingsStorageStore((state) => state.loading)
  const storageSaveFailed = useSettingsStorageStore((state) => state.saveFailed)
  const cacheAlertIdentity = cacheAlertIdentityKey(me)
  const initializeCacheAlertIdentity = useSettingsCacheAlertStore(
    (state) => state.initializeIdentity,
  )
  const resetCacheAlertIdentity = useSettingsCacheAlertStore(
    (state) => state.resetIdentity,
  )
  const ensureCacheAlertLoaded = useSettingsCacheAlertStore(
    (state) => state.ensureLoaded,
  )
  const refreshCacheAlert = useSettingsCacheAlertStore((state) => state.refresh)
  const cacheAlertForm = useSettingsCacheAlertStore((state) => state.form)
  const cacheAlertBaseline = useSettingsCacheAlertStore(
    (state) => state.baseline,
  )
  const cacheAlertLoaded = useSettingsCacheAlertStore((state) => state.loaded)
  const cacheAlertLoading = useSettingsCacheAlertStore((state) => state.loading)
  const cacheAlertSaveFailed = useSettingsCacheAlertStore(
    (state) => state.saveFailed,
  )
  const configArchiveIdentity = configArchiveIdentityKey(me)
  const initializeConfigArchiveIdentity = useSettingsConfigArchiveStore(
    (state) => state.initializeIdentity,
  )
  const resetConfigArchiveIdentity = useSettingsConfigArchiveStore(
    (state) => state.resetIdentity,
  )
  const { pendingActions } = useAsyncAction()

  const requestsActive = requested === 'requests' && tab === 'requests'
  const networkActive = requested === 'network' && tab === 'network'
  const reviewActive = requested === 'review' && tab === 'review'
  const storageActive = requested === 'storage' && tab === 'storage'
  const cacheAlertActive = requested === 'cache-alert' && tab === 'cache-alert'
  const configArchiveActive =
    requested === 'config-archive' && tab === 'config-archive'

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

  useEffect(() => {
    if (!reviewActive || me === null) return
    initializeReviewIdentity(me)
  }, [reviewActive, me, initializeReviewIdentity])

  useEffect(() => {
    if (reviewActive || me === null) return
    resetReviewIdentity(null)
  }, [reviewActive, me, resetReviewIdentity])

  useEffect(() => {
    if (!reviewActive || reviewIdentity === '') return
    void ensureReviewLoaded(queryClient)
  }, [reviewActive, reviewIdentity, ensureReviewLoaded, queryClient])

  useEffect(() => {
    if (!storageActive || me === null) return
    initializeStorageIdentity(me)
  }, [storageActive, me, initializeStorageIdentity])

  useEffect(() => {
    if (storageActive || me === null) return
    resetStorageIdentity(null)
  }, [storageActive, me, resetStorageIdentity])

  useEffect(() => {
    if (!storageActive || storageIdentity === '') return
    void ensureStorageLoaded(queryClient)
  }, [storageActive, storageIdentity, ensureStorageLoaded, queryClient])

  useEffect(() => {
    if (!cacheAlertActive || me === null) return
    initializeCacheAlertIdentity(me)
  }, [cacheAlertActive, me, initializeCacheAlertIdentity])

  useEffect(() => {
    if (cacheAlertActive || me === null) return
    resetCacheAlertIdentity(null)
  }, [cacheAlertActive, me, resetCacheAlertIdentity])

  useEffect(() => {
    if (!cacheAlertActive || cacheAlertIdentity === '') return
    void ensureCacheAlertLoaded(queryClient)
  }, [
    cacheAlertActive,
    cacheAlertIdentity,
    ensureCacheAlertLoaded,
    queryClient,
  ])

  useEffect(() => {
    if (!requestsActive || me === null) return
    initializeIdentity(me)
  }, [requestsActive, me, initializeIdentity])

  useEffect(() => {
    if (!requestsActive || identityKey === '') return
    void ensureLoaded(queryClient)
  }, [requestsActive, identityKey, ensureLoaded, queryClient])

  useEffect(() => {
    if (!networkActive || me === null) return
    initializeNetworkIdentity(me)
  }, [networkActive, me, initializeNetworkIdentity])

  useEffect(() => {
    if (!networkActive || networkIdentity === '') return
    void ensureNetworkLoaded(queryClient)
  }, [networkActive, networkIdentity, ensureNetworkLoaded, queryClient])

  if (requestsActive) {
    const refreshBlocked =
      !loaded ||
      settingsRefreshBlocked({ pendingActions, dirty, failed }) ||
      loading
    return (
      <section
        aria-label={t('settingsUsage')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-REQUESTS"
        data-settings-tab={tab}
      >
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <h1 className="m-0 text-lg font-semibold">{t('settingsUsage')}</h1>
          <Button
            aria-label={t('refresh')}
            disabled={refreshBlocked}
            id="settings-requests-refresh"
            onClick={() => void refresh(queryClient)}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </div>
        <SettingsRequestsTab />
      </section>
    )
  }

  if (networkActive) {
    const networkDirty =
      relayWhitelistDirtyFields(networkForm, networkBaseline).length > 0
    const refreshBlocked = relayWhitelistRefreshBlocked({
      pendingActions,
      dirty: networkDirty,
      failed: networkSaveFailed,
      loading: networkLoading,
      loaded: networkLoaded,
    })
    return (
      <section
        aria-label={t('settingsNetwork')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-NETWORK"
        data-settings-tab={tab}
      >
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <h1 className="m-0 text-lg font-semibold">{t('settingsNetwork')}</h1>
          <Button
            aria-label={t('refresh')}
            disabled={refreshBlocked}
            id="settings-network-refresh"
            onClick={() => void refreshNetwork(queryClient)}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </div>
        <SettingsNetworkTab />
      </section>
    )
  }

  if (reviewActive) {
    const reviewDirty =
      llmReviewDirtyFields(reviewForm, reviewBaseline).length > 0
    const refreshBlocked = llmReviewRefreshBlocked({
      pendingActions,
      dirty: reviewDirty,
      failed: reviewSaveFailed,
      loading: reviewLoading,
      loaded: reviewLoaded,
    })
    return (
      <section
        aria-label={t('settingsReview')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-REVIEW"
        data-settings-tab={tab}
      >
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <h1 className="m-0 text-lg font-semibold">{t('settingsReview')}</h1>
          <Button
            aria-label={t('refresh')}
            disabled={refreshBlocked}
            id="settings-review-refresh"
            onClick={() => void refreshReview(queryClient)}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </div>
        <SettingsReviewTab />
      </section>
    )
  }

  if (storageActive) {
    const refreshBlocked = storageRefreshBlocked({
      pendingActions,
      dirty: rawObjectStoreDirty(storageForm, storageBaseline),
      failed: storageSaveFailed,
      loading: storageLoading,
      loaded: storageLoaded,
    })
    return (
      <section
        aria-label={t('settingsStorage')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-STORAGE"
        data-settings-tab={tab}
      >
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <h1 className="m-0 text-lg font-semibold">{t('settingsStorage')}</h1>
          <Button
            aria-label={t('refresh')}
            disabled={refreshBlocked}
            id="settings-storage-refresh"
            onClick={() => void refreshStorage(queryClient)}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </div>
        <SettingsStorageTab />
      </section>
    )
  }

  if (cacheAlertActive) {
    const refreshBlocked = cacheAlertRefreshBlocked({
      pendingActions,
      dirty: cacheAlertDirty(cacheAlertForm, cacheAlertBaseline),
      failed: cacheAlertSaveFailed,
      loading: cacheAlertLoading,
      loaded: cacheAlertLoaded,
    })
    return (
      <section
        aria-label={t('settingsCacheAlert')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-CACHE-ALERT"
        data-settings-tab={tab}
      >
        <div className="flex min-w-0 flex-wrap items-center justify-between gap-2">
          <h1 className="m-0 text-lg font-semibold">
            {t('settingsCacheAlert')}
          </h1>
          <Button
            aria-label={t('refresh')}
            disabled={refreshBlocked}
            id="settings-cache-alert-refresh"
            onClick={() => void refreshCacheAlert(queryClient)}
            size="sm"
            type="button"
            variant="outline"
          >
            <RefreshCw aria-hidden="true" />
            {t('refresh')}
          </Button>
        </div>
        <SettingsCacheAlertTab />
      </section>
    )
  }

  if (configArchiveActive) {
    return (
      <section
        aria-label={t('settingsConfigArchive')}
        className="grid min-w-0 gap-3"
        data-foundation-route-id="R-SETTINGS-CONFIG-ARCHIVE"
        data-settings-tab={tab}
      >
        <h1 className="m-0 text-lg font-semibold">
          {t('settingsConfigArchive')}
        </h1>
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
      <h1 className="text-lg font-semibold">{t('settingsGeneral')}</h1>
      <SettingsGeneralTab />
    </section>
  )
}
