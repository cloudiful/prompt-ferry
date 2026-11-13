import { useForm, useStore } from '@tanstack/react-form'
import type { FormValidateOrFn } from '@tanstack/form-core'
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
  initialForm: EndpointForm
  onOpenChange: (open: boolean) => void
  onSubmit: (form: EndpointForm) => void | Promise<void>
  open: boolean
  pending: boolean
  runOAuthAction: RunOAuthAction
  users: User[]
}

export function EndpointDialog({
  dialogGeneration,
  endpointId,
  initialForm,
  onOpenChange,
  onSubmit,
  open,
  pending,
  runOAuthAction,
  users,
}: EndpointDialogProps) {
  const { t } = useLocale()
  const userPlanRevisionRef = useRef(0)
  const providerPlanRef = useRef({
    provider: initialForm.provider,
    plan: initialForm.plan,
  })
  const [view, setView] = useState<'main' | 'settings'>('main')

  const form = useForm<
    EndpointForm,
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    FormValidateOrFn<EndpointForm>,
    undefined,
    undefined,
    undefined,
    undefined,
    never
  >({
    defaultValues: initialForm,
    onSubmit: async ({ value }) => {
      await onSubmit(value)
    },
    validators: {
      onSubmit: ({ value }) => {
        if (
          value.scope === 'user' &&
          (value.owner_user_id === null ||
            !users.some((user) => user.user_id === value.owner_user_id))
        ) {
          return t('ownerRequired')
        }
        if (!isProxyUrlValid(value.proxy_url)) return t('proxyAddress')
        if (!value.active_windows.every(isScheduleRowValid)) {
          return t('scheduleInvalid')
        }
        return undefined
      },
    },
  })
  const values = useStore(form.store, (state) => state.values)
  const submitAttempts = useStore(
    form.store,
    (state) => state.submissionAttempts,
  )
  const formAccepted = useStore(
    form.store,
    (state) => typeof state.errorMap.onSubmit === 'undefined',
  )
  const submitError =
    submitAttempts > 0 && typeof form.store.state.errorMap.onSubmit === 'string'
      ? form.store.state.errorMap.onSubmit
      : undefined

  const isSubscription =
    normalizeProviderPlan(values.provider, values.plan) ===
    'chatgpt_subscription'
  const serviceTierEligible = supportsServiceTierFor(
    endpointFormProtocol(values),
  )
  const validOwner =
    values.scope !== 'user' ||
    (values.owner_user_id !== null &&
      users.some((user) => user.user_id === values.owner_user_id))
  const clientValid =
    submitAttempts === 0
      ? validOwner &&
        isProxyUrlValid(values.proxy_url) &&
        values.active_windows.every(isScheduleRowValid)
      : formAccepted
  const canSave = !pending && clientValid
  const hasSettings =
    values.proxy_url.trim() !== '' ||
    values.has_saved_proxy_url ||
    values.active_windows.length > 0 ||
    (values.service_tier ?? '').trim() !== ''

  useEffect(() => {
    if (!open) setView('main')
  }, [open])

  useEffect(() => {
    const previous = providerPlanRef.current
    if (
      previous.provider !== values.provider ||
      previous.plan !== values.plan
    ) {
      userPlanRevisionRef.current += 1
      providerPlanRef.current = {
        provider: values.provider,
        plan: values.plan,
      }
    }
  }, [values.provider, values.plan])

  function captureStatusSnapshot(): OAuthStatusSnapshot {
    return {
      endpointId: endpointId ?? '',
      dialogGeneration,
      provider: values.provider,
      plan: values.plan,
      has_oauth_token: values.has_oauth_token,
      userPlanRevision: userPlanRevisionRef.current,
    }
  }

  function isOAuthSnapshotCurrent(snapshot: OAuthStatusSnapshot): boolean {
    return (
      open &&
      endpointId !== null &&
      snapshot.endpointId === endpointId &&
      snapshot.dialogGeneration === dialogGeneration &&
      values.provider === 'openai'
    )
  }

  function applyOAuthStatus(
    snapshot: OAuthStatusSnapshot,
    status: { has_oauth_token: boolean; plan: EndpointForm['plan'] },
  ): void {
    if (!isOAuthSnapshotCurrent(snapshot)) return
    const next = applyOAuthStatusToForm(values, snapshot, status, {
      endpointId: endpointId ?? '',
      dialogGeneration,
      userPlanRevision: userPlanRevisionRef.current,
    })
    if (!next || next === values) return
    providerPlanRef.current = {
      provider: next.provider,
      plan: next.plan,
    }
    form.setFieldValue('has_oauth_token', next.has_oauth_token)
    form.setFieldValue('plan', next.plan)
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
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            if (pending) return
            void form.handleSubmit()
          }}
        >
          {submitError ? (
            <p className="text-xs text-destructive" role="alert">
              {submitError}
            </p>
          ) : null}
          <fieldset className="grid min-w-0 gap-4" disabled={pending}>
            {view === 'main' ? (
              <>
                <EndpointProviderFields form={form} users={users} />
                {!isSubscription ? <EndpointApiKeysEditor form={form} /> : null}
                {values.provider === 'openai' ? (
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
                  {values.provider === 'minimax' ? (
                    <label className="inline-flex min-h-8 items-center gap-2 text-sm">
                      <span>{t('minimaxMcp')}</span>
                      <Switch
                        aria-label={t('minimaxMcp')}
                        checked={values.mcp_enabled}
                        onCheckedChange={(checked) =>
                          form.setFieldValue('mcp_enabled', checked === true)
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
                      hasSaved={values.has_saved_proxy_url}
                      onChange={({ proxyUrl, hasSaved }) => {
                        form.setFieldValue('proxy_url', proxyUrl)
                        form.setFieldValue('has_saved_proxy_url', hasSaved)
                      }}
                      proxyUrl={values.proxy_url}
                    />
                  </SettingsFieldRow>
                  <SettingsFieldRow
                    hint={t('scheduleWindowsHint')}
                    label={t('scheduleWindows')}
                  >
                    <ScheduleWindowsFields
                      onChange={(windows, touched) => {
                        form.setFieldValue('active_windows', windows)
                        form.setFieldValue(
                          'active_windows_touched',
                          values.active_windows_touched || touched,
                        )
                      }}
                      windows={values.active_windows}
                    />
                  </SettingsFieldRow>
                  {serviceTierEligible ? (
                    <ServiceTierOverrideField
                      onChange={(value) =>
                        form.setFieldValue('service_tier', value)
                      }
                      value={values.service_tier}
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
