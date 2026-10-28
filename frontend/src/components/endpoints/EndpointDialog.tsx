import { ArrowLeft, LoaderCircle, Save, Settings2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { EndpointApiKeysEditor } from '@/components/endpoints/EndpointApiKeysEditor'
import { EndpointProviderFields } from '@/components/endpoints/EndpointProviderFields'
import { EndpointOAuthSection } from '@/components/endpoints/EndpointOAuthSection'
import {
  ProxySettingsFields,
  isProxyUrlValid,
} from '@/components/shared/ProxySettingsFields'
import { ScheduleWindowsFields } from '@/components/shared/ScheduleWindowsFields'
import { ServiceTierOverrideField } from '@/components/shared/ServiceTierOverrideField'
import { SettingsFieldRow } from '@/components/shared/SettingsFieldRow'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import type { User } from '@/generated/admin-api'
import { useLocale } from '@/lib/i18n'
import type { EndpointForm } from '@/models'
import {
  applyOAuthStatusToForm,
  type OAuthStatusSnapshot,
  type RunOAuthAction,
} from '@/models/endpoints/oauth'
import { normalizeProviderPlan } from '@/admin-mappers/forms/endpoint'
import {
  endpointFormProtocol,
  supportsServiceTierFor,
} from '@/models/endpoints/service-tier'
import { isScheduleRowValid } from '@/models/endpoints/schedule-windows'

type EndpointDialogProps = {
  dialogGeneration: number
  endpointId: string | null
  form: EndpointForm
  onChange: (update: (current: EndpointForm) => EndpointForm) => void
  onOpenChange: (open: boolean) => void
  onSubmit: () => void
  open: boolean
  pending: boolean
  runOAuthAction: RunOAuthAction
  users: User[]
}

export function EndpointDialog({
  dialogGeneration,
  endpointId,
  form,
  onChange,
  onOpenChange,
  onSubmit,
  open,
  pending,
  runOAuthAction,
  users,
}: EndpointDialogProps) {
  const { t } = useLocale()
  const formRef = useRef(form)
  const userPlanRevisionRef = useRef(0)
  const identityRef = useRef({ dialogGeneration, endpointId })
  formRef.current = form
  if (
    identityRef.current.dialogGeneration !== dialogGeneration ||
    identityRef.current.endpointId !== endpointId
  ) {
    identityRef.current = { dialogGeneration, endpointId }
    userPlanRevisionRef.current = 0
  }
  const [view, setView] = useState<'main' | 'settings'>('main')
  const isSubscription =
    normalizeProviderPlan(form.provider, form.plan) === 'chatgpt_subscription'
  const serviceTierEligible = supportsServiceTierFor(endpointFormProtocol(form))
  const validOwner =
    form.scope !== 'user' ||
    (form.owner_user_id !== null &&
      users.some((user) => user.user_id === form.owner_user_id))
  const canSave =
    !pending &&
    validOwner &&
    isProxyUrlValid(form.proxy_url) &&
    form.active_windows.every(isScheduleRowValid)
  const hasSettings =
    form.proxy_url.trim() !== '' ||
    form.has_saved_proxy_url ||
    form.active_windows.length > 0 ||
    (form.service_tier ?? '').trim() !== ''

  useEffect(() => {
    if (!open) setView('main')
  }, [open])

  function updateForm(update: (current: EndpointForm) => EndpointForm): void {
    const current = formRef.current
    const next = update(current)
    if (current.provider !== next.provider || current.plan !== next.plan) {
      userPlanRevisionRef.current += 1
    }
    formRef.current = next
    onChange(() => next)
  }

  function captureStatusSnapshot(): OAuthStatusSnapshot {
    const current = formRef.current
    return {
      endpointId: endpointId ?? '',
      dialogGeneration,
      provider: current.provider,
      plan: current.plan,
      has_oauth_token: current.has_oauth_token,
      userPlanRevision: userPlanRevisionRef.current,
    }
  }

  function isOAuthSnapshotCurrent(snapshot: OAuthStatusSnapshot): boolean {
    return (
      open &&
      endpointId !== null &&
      snapshot.endpointId === endpointId &&
      snapshot.dialogGeneration === dialogGeneration &&
      formRef.current.provider === 'openai'
    )
  }

  function applyOAuthStatus(
    snapshot: OAuthStatusSnapshot,
    status: { has_oauth_token: boolean; plan: EndpointForm['plan'] },
  ): void {
    if (!isOAuthSnapshotCurrent(snapshot)) return
    const current = formRef.current
    const next = applyOAuthStatusToForm(current, snapshot, status, {
      endpointId: endpointId ?? '',
      dialogGeneration,
      userPlanRevision: userPlanRevisionRef.current,
    })
    if (!next || next === current) return
    formRef.current = next
    onChange(() => next)
  }

  return (
    <Dialog
      onOpenChange={(nextOpen) => !pending && onOpenChange(nextOpen)}
      open={open}
    >
      <DialogContent
        className="max-h-[calc(100vh-2rem)] overflow-y-auto sm:max-w-[min(calc(100vw_-_2rem),64rem)]"
        onEscapeKeyDown={(event) => {
          if (pending) event.preventDefault()
        }}
        onPointerDownOutside={(event) => {
          if (pending) event.preventDefault()
        }}
        showCloseButton={!pending}
      >
        <DialogHeader>
          <DialogTitle>
            {endpointId ? t('editEndpoint') : t('newEndpoint')}
          </DialogTitle>
        </DialogHeader>
        <form
          className="grid min-w-0 gap-4"
          onSubmit={(event) => {
            event.preventDefault()
            if (canSave) onSubmit()
          }}
        >
          <fieldset className="grid min-w-0 gap-4" disabled={pending}>
            {view === 'main' ? (
              <>
                <EndpointProviderFields
                  form={form}
                  onChange={updateForm}
                  users={users}
                />
                {!isSubscription ? (
                  <EndpointApiKeysEditor form={form} onChange={updateForm} />
                ) : null}
                {form.provider === 'openai' ? (
                  <div className="border-t pt-3">
                    {endpointId ? (
                      <EndpointOAuthSection
                        actionPending={pending}
                        captureStatusSnapshot={captureStatusSnapshot}
                        dialogGeneration={dialogGeneration}
                        endpointId={endpointId}
                        isSnapshotCurrent={isOAuthSnapshotCurrent}
                        onStatus={applyOAuthStatus}
                        runAction={runOAuthAction}
                      />
                    ) : (
                      <p className="text-xs text-muted-foreground">
                        {t('endpointOAuthSaveFirst')}
                      </p>
                    )}
                  </div>
                ) : null}
                <div className="flex flex-wrap items-center justify-between gap-3 border-t pt-3">
                  {form.provider === 'minimax' ? (
                    <label className="inline-flex min-h-8 items-center gap-2 text-sm">
                      <span>{t('minimaxMcp')}</span>
                      <Switch
                        aria-label={t('minimaxMcp')}
                        checked={form.mcp_enabled}
                        onCheckedChange={(checked) =>
                          updateForm((current) => ({
                            ...current,
                            mcp_enabled: checked,
                          }))
                        }
                      />
                    </label>
                  ) : (
                    <span />
                  )}
                  <Button
                    aria-pressed={hasSettings}
                    onClick={() => setView('settings')}
                    size="sm"
                    type="button"
                    variant={hasSettings ? 'secondary' : 'outline'}
                  >
                    <Settings2 aria-hidden="true" />
                    {t('endpointSettings')}
                  </Button>
                </div>
              </>
            ) : (
              <>
                <Button
                  aria-label={t('cancel')}
                  className="justify-self-start"
                  onClick={() => setView('main')}
                  size="sm"
                  type="button"
                  variant="ghost"
                >
                  <ArrowLeft aria-hidden="true" />
                  {t('endpointSettings')}
                </Button>
                <div className="grid gap-4 rounded-lg border bg-muted/30 p-3">
                  <SettingsFieldRow
                    hint={t('proxyUrlHint')}
                    label={t('proxyUrl')}
                  >
                    <ProxySettingsFields
                      hasSaved={form.has_saved_proxy_url}
                      onChange={({ proxyUrl, hasSaved }) =>
                        updateForm((current) => ({
                          ...current,
                          proxy_url: proxyUrl,
                          has_saved_proxy_url: hasSaved,
                        }))
                      }
                      proxyUrl={form.proxy_url}
                    />
                  </SettingsFieldRow>
                  <SettingsFieldRow
                    hint={t('scheduleWindowsHint')}
                    label={t('scheduleWindows')}
                  >
                    <ScheduleWindowsFields
                      onChange={(windows, touched) =>
                        updateForm((current) => ({
                          ...current,
                          active_windows: windows,
                          active_windows_touched:
                            current.active_windows_touched || touched,
                        }))
                      }
                      windows={form.active_windows}
                    />
                  </SettingsFieldRow>
                  {serviceTierEligible ? (
                    <ServiceTierOverrideField
                      onChange={(value) =>
                        updateForm((current) => ({
                          ...current,
                          service_tier: value,
                        }))
                      }
                      value={form.service_tier}
                    />
                  ) : null}
                </div>
              </>
            )}
          </fieldset>
          <DialogFooter>
            <Button
              disabled={pending}
              onClick={() => onOpenChange(false)}
              type="button"
              variant="outline"
            >
              {t('cancel')}
            </Button>
            <Button disabled={!canSave} type="submit">
              {pending ? (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              ) : (
                <Save aria-hidden="true" />
              )}
              {t('saveEndpoint')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
