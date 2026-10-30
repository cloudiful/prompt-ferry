import { useEffect } from 'react'
import { useLocation } from 'react-router'
import { LoaderCircle, RefreshCw, Save } from 'lucide-react'
import { RedactionPreviewPanel } from '@/components/redaction/RedactionPreviewPanel'
import { RedactionRulesPanel } from '@/components/redaction/RedactionRulesPanel'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  redactionIdentityKey,
  redactionTargetToken,
  useRedactionStore,
} from '@/lib/redaction'
import { usePageTitle } from '@/lib/router'

export function RedactionPage() {
  const location = useLocation()
  const isPreview = location.pathname === '/redaction/preview'
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const identityKey = redactionIdentityKey(me)
  const userId = me?.user_id ?? null
  const isAdmin = me?.is_admin === true
  const redaction = useRedactionStore()
  usePageTitle(isPreview ? 'redactionPreview' : 'redactionRules')

  const currentTargetToken = redactionTargetToken(redaction)
  const identityReady =
    identityKey !== null && redaction.identityKey === identityKey
  const ready =
    identityReady && redaction.loadedTargetToken === currentTargetToken

  useEffect(() => {
    if (!me || identityKey === null) return
    const store = useRedactionStore.getState()
    store.initializeIdentity(me)
    const current = useRedactionStore.getState()
    if (current.loadedTargetToken === redactionTargetToken(current)) return
    let active = true

    async function load(): Promise<void> {
      if (isAdmin && current.userOptions.length === 0) {
        try {
          await store.loadUserOptions()
        } catch (cause) {
          if (active) notifyApiError(cause)
        }
      }
      try {
        await useRedactionStore.getState().refresh()
      } catch (cause) {
        if (active) notifyApiError(cause)
      }
    }

    void load()
    return () => {
      active = false
    }
  }, [identityKey, isAdmin, me, notifyApiError])

  async function refresh(): Promise<void> {
    try {
      await useRedactionStore.getState().refresh()
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function save(): Promise<void> {
    try {
      const outcome = await useRedactionStore.getState().save()
      if (outcome.executed) notifySuccess(t('saved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function runPreview(): Promise<void> {
    try {
      await useRedactionStore.getState().runPreview()
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function changeScope(scope: string): void {
    if (scope !== 'global' && scope !== 'user') return
    const state = useRedactionStore.getState()
    const targetUserId =
      scope === 'user'
        ? (state.targetUserId ?? state.userOptions[0]?.user_id ?? userId)
        : null
    state.setTarget(scope, targetUserId)
    void refresh()
  }

  function changeTargetUser(value: string): void {
    const nextUserId = Number(value)
    if (!Number.isSafeInteger(nextUserId) || nextUserId <= 0) return
    useRedactionStore.getState().setTarget('user', nextUserId)
    void refresh()
  }

  return (
    <div
      className="grid min-w-0 gap-5"
      data-foundation-route-id={
        isPreview ? 'R-REDACTION-PREVIEW' : 'R-REDACTION-RULES'
      }
    >
      <HeaderActions>
        <div className="flex min-w-0 flex-wrap items-end gap-2">
          <div className="flex min-h-9 items-center gap-2">
            <span className="text-sm text-muted-foreground">
              {t('redactionRuleScope')}
            </span>
            {isAdmin ? (
              <Select
                disabled={
                  !identityReady || redaction.loading || redaction.saving
                }
                onValueChange={changeScope}
                value={redaction.scope}
              >
                <SelectTrigger
                  aria-label={t('redactionRuleScope')}
                  className="w-32"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="global">
                    {t('redactionPublicRules')}
                  </SelectItem>
                  <SelectItem value="user">
                    {t('redactionPrivateRules')}
                  </SelectItem>
                </SelectContent>
              </Select>
            ) : (
              <span aria-label={t('redactionRuleScope')} className="text-sm">
                {t('redactionMyPrivateRules')}
              </span>
            )}
          </div>

          {isAdmin && redaction.scope === 'user' && (
            <div className="flex min-h-9 items-center gap-2">
              <span className="text-sm text-muted-foreground">
                {t('redactionTargetUser')}
              </span>
              <Select
                disabled={
                  !identityReady ||
                  redaction.loading ||
                  redaction.saving ||
                  redaction.userOptionsLoading ||
                  redaction.userOptions.length === 0
                }
                onValueChange={changeTargetUser}
                value={
                  redaction.targetUserId === null
                    ? undefined
                    : String(redaction.targetUserId)
                }
              >
                <SelectTrigger
                  aria-label={t('redactionTargetUser')}
                  className="w-52 max-w-[calc(100vw-12rem)]"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {redaction.userOptions.map((user) => (
                    <SelectItem key={user.user_id} value={String(user.user_id)}>
                      {`${user.display_name || user.login_name} · ${user.login_name}`}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          <Button
            disabled={!ready || redaction.loading || redaction.saving}
            onClick={() => void save()}
            type="button"
          >
            {redaction.saving ? (
              <LoaderCircle aria-hidden="true" className="animate-spin" />
            ) : (
              <Save aria-hidden="true" />
            )}
            {t('save')}
          </Button>
          <Button
            disabled={redaction.saving}
            onClick={() => void refresh()}
            type="button"
            variant="outline"
          >
            {redaction.loading ? (
              <LoaderCircle aria-hidden="true" className="animate-spin" />
            ) : (
              <RefreshCw aria-hidden="true" />
            )}
            {t('refresh')}
          </Button>
          <label className="flex min-h-9 items-center gap-2 rounded-md border px-3 text-sm">
            <Checkbox
              aria-label={t('redaction')}
              checked={redaction.config.enabled}
              disabled={!ready}
              onCheckedChange={(checked) =>
                useRedactionStore.getState().setEnabled(checked === true)
              }
            />
            <span>{t('redaction')}</span>
          </label>
        </div>
      </HeaderActions>

      {!ready ? (
        redaction.loadError ? (
          <div className="flex flex-wrap items-center gap-3" role="alert">
            <span>{t('failed')}</span>
            <Button
              onClick={() => void refresh()}
              type="button"
              variant="outline"
            >
              {t('refresh')}
            </Button>
          </div>
        ) : (
          <div
            aria-label={t('loading')}
            className="grid min-h-32 place-items-center"
            role="status"
          >
            <LoaderCircle aria-hidden="true" className="size-6 animate-spin" />
          </div>
        )
      ) : isPreview ? (
        <RedactionPreviewPanel
          onRunPreview={() => void runPreview()}
          previewing={redaction.previewing}
        />
      ) : (
        <RedactionRulesPanel />
      )}
    </div>
  )
}
