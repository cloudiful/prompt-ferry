import { useForm, useStore } from '@tanstack/react-form'
import type { FormValidateOrFn } from '@tanstack/form-core'
import {
  ArrowUp,
  GripVertical,
  LoaderCircle,
  Plus,
  Settings2,
  Trash2,
} from 'lucide-react'
import { useRef, useState } from 'react'
import type { NativeApi, ProviderEndpoint, User } from '@/generated/admin-api'
import { createEmptyModelRouteForm } from '@/admin-mappers/forms/model-route'
import type { ModelRouteForm, ModelRouteTargetForm } from '@/models'
import type { EndpointOption } from '@/models/endpoints'
import { EndpointModelInput } from '@/components/endpoints/EndpointModelInput'
import { ModelRouteTargetSettingsPage } from '@/components/endpoints/ModelRouteTargetSettingsPage'
import { ProviderIcon } from '@/components/providers/ProviderIcon'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import { useModelRouteTargetDrag } from '@/lib/use-model-route-target-drag'
import {
  canSaveTargets,
  hasTargetProxy,
  hasTargetSchedule,
  hasTargetSettings,
  suggestProtocolForTarget,
  targetProxySummary,
  targetScheduleSummary,
} from '@/models/endpoints/model-route-target'
import { normalizeNativeApiAxis } from '@/models/endpoints/service-tier'

const NATIVE_API_LABELS: Record<
  NativeApi,
  | 'endpointSourceAuto'
  | 'nativeApiAnthropicMessages'
  | 'nativeApiChat'
  | 'nativeApiEmbeddings'
  | 'nativeApiRealtime'
  | 'nativeApiResponses'
> = {
  auto: 'endpointSourceAuto',
  anthropic_messages: 'nativeApiAnthropicMessages',
  chat: 'nativeApiChat',
  embeddings: 'nativeApiEmbeddings',
  realtime: 'nativeApiRealtime',
  responses: 'nativeApiResponses',
}

type ModelRouteDialogProps = {
  endpoints: ProviderEndpoint[]
  endpointOptions: EndpointOption[]
  initialForm: ModelRouteForm
  onOpenChange: (open: boolean) => void
  onSave: (form: ModelRouteForm) => void | Promise<void>
  open: boolean
  ownerOptionsLoading: boolean
  savePending: boolean
  users: User[]
}

export function ModelRouteDialog({
  endpoints,
  endpointOptions,
  initialForm,
  onOpenChange,
  onSave,
  open,
  ownerOptionsLoading,
  savePending,
  users,
}: ModelRouteDialogProps) {
  const { t } = useLocale()
  const [settingsTargetIndex, setSettingsTargetIndex] = useState<number | null>(
    null,
  )
  const targetKeys = useRef(new WeakMap<ModelRouteTargetForm, string>())
  const nextTargetKey = useRef(0)

  const form = useForm<
    ModelRouteForm,
    undefined,
    undefined,
    undefined,
    undefined,
    undefined,
    FormValidateOrFn<ModelRouteForm>,
    undefined,
    undefined,
    undefined,
    undefined,
    never
  >({
    defaultValues: initialForm,
    onSubmit: async ({ value }) => {
      await onSave(value)
    },
    validators: {
      onSubmit: ({ value }) => {
        if (value.model_pattern.trim() === '') return t('modelPatternRequired')
        if (value.targets.length === 0) return t('targetRequired')
        if (value.targets.some((target) => target.endpoint_id.trim() === '')) {
          return t('targetEndpointRequired')
        }
        if (value.scope === 'user' && value.owner_user_id === null) {
          return t('ownerRequired')
        }
        if (!canSaveTargets(value.targets)) return t('targetInvalid')
        return undefined
      },
    },
  })
  const values = useStore(form.store, (state) => state.values)
  const orderedTargets = values.targets
  const drag = useModelRouteTargetDrag(orderedTargets, (targets) =>
    form.setFieldValue('targets', targets),
  )
  const selectedTarget =
    settingsTargetIndex === null
      ? null
      : (orderedTargets[settingsTargetIndex] ?? null)
  const canSubmit = useStore(form.store, (state) => {
    const value = state.values
    const attempts = state.submissionAttempts
    // After a rejected submit attempt the Form owns the gate: the operator
    // corrects the validation error it reported, not a silently disabled
    // button.
    if (attempts > 0) return typeof state.errorMap.onSubmit === 'undefined'
    return (
      value.model_pattern.trim() !== '' &&
      value.targets.length > 0 &&
      value.targets.every((target) => target.endpoint_id.trim() !== '') &&
      (value.scope !== 'user' || value.owner_user_id !== null) &&
      canSaveTargets(value.targets)
    )
  })
  const submitError = useStore(form.store, (state) =>
    state.submissionAttempts > 0 && typeof state.errorMap.onSubmit === 'string'
      ? state.errorMap.onSubmit
      : undefined,
  )

  function targetKey(target: ModelRouteTargetForm): string {
    const current = targetKeys.current.get(target)
    if (current) return current
    const next = `route-target-${++nextTargetKey.current}`
    targetKeys.current.set(target, next)
    return next
  }

  function updateTarget(
    index: number,
    patch: Partial<ModelRouteTargetForm>,
  ): void {
    const target = orderedTargets[index]
    if (!target) return
    Object.assign(target, patch)
    form.setFieldValue('targets', [...orderedTargets])
  }

  function endpointOptionFor(endpointId: string): EndpointOption | undefined {
    return endpointOptions.find((option) => option.value === endpointId)
  }

  function endpointProtocolFor(endpointId: string): NativeApi {
    return normalizeNativeApiAxis(
      endpoints.find((endpoint) => endpoint.endpoint_id === endpointId)
        ?.native_api,
    )
  }

  function suggestionProtocol(
    target: ModelRouteTargetForm,
  ): NativeApi | undefined {
    return suggestProtocolForTarget(target, endpointProtocolFor)
  }

  function backToTargets(): void {
    setSettingsTargetIndex(null)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[min(94vh,58rem)] overflow-y-auto sm:max-w-[min(64rem,calc(100vw_-_2rem))]"
        onEscapeKeyDown={(event) => {
          if (savePending) event.preventDefault()
        }}
        onInteractOutside={(event) => {
          if (savePending) event.preventDefault()
        }}
        showCloseButton={!savePending}
      >
        <DialogHeader>
          <DialogTitle>
            {t(values.rule_id ? 'editModelRoute' : 'newModelRoute')}
          </DialogTitle>
        </DialogHeader>
        <form
          autoComplete="off"
          className="grid min-w-0 gap-4"
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            if (savePending) return
            void form.handleSubmit()
          }}
        >
          <fieldset className="grid min-w-0 gap-4" disabled={savePending}>
            {submitError ? (
              <p className="text-xs text-destructive" role="alert">
                {submitError}
              </p>
            ) : null}
            {selectedTarget && settingsTargetIndex !== null ? (
              <>
                <div className="flex min-w-0 items-center gap-2">
                  <Button
                    aria-label={t('cancel')}
                    onClick={backToTargets}
                    size="icon-sm"
                    type="button"
                    variant="ghost"
                  >
                    <ArrowUp className="rotate-[-90deg]" />
                  </Button>
                  <h3 className="font-medium">{t('targetSettings')}</h3>
                  <span className="min-w-0 truncate text-sm text-muted-foreground">
                    {endpoints.find(
                      (endpoint) =>
                        endpoint.endpoint_id === selectedTarget.endpoint_id,
                    )?.name ?? selectedTarget.endpoint_id}
                  </span>
                </div>
                <ModelRouteTargetSettingsPage
                  endpointProtocol={endpointProtocolFor(
                    selectedTarget.endpoint_id,
                  )}
                  onChange={(value) => updateTarget(settingsTargetIndex, value)}
                  target={selectedTarget}
                />
              </>
            ) : (
              <>
                <div className="grid min-w-0 gap-3 sm:grid-cols-[9rem_minmax(0,1fr)_auto] sm:items-end">
                  <label className="grid gap-1 text-xs font-medium">
                    <span>{t('scope')}</span>
                    <Select
                      onValueChange={(value) => {
                        form.setFieldValue(
                          'scope',
                          value === 'user' ? 'user' : 'admin',
                        )
                        form.setFieldValue(
                          'owner_user_id',
                          value === 'user' ? values.owner_user_id : null,
                        )
                      }}
                      value={values.scope}
                    >
                      <SelectTrigger aria-label={t('scope')}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="admin">{t('scopeAdmin')}</SelectItem>
                        <SelectItem value="user">{t('scopeUser')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </label>
                  <label className="grid min-w-0 gap-1 text-xs font-medium">
                    <span>{t('modelPattern')}</span>
                    <form.Field name="model_pattern">
                      {(field) => (
                        <Input
                          aria-label={t('modelPattern')}
                          className="min-w-0"
                          onChange={(event) =>
                            field.handleChange(event.currentTarget.value)
                          }
                          value={field.state.value}
                        />
                      )}
                    </form.Field>
                  </label>
                  <div className="flex items-center gap-2 pb-2">
                    <span className="text-xs font-medium">{t('status')}</span>
                    <form.Field name="enabled">
                      {(field) => (
                        <Switch
                          aria-label={t('status')}
                          checked={field.state.value}
                          onCheckedChange={(enabled) =>
                            field.handleChange(enabled === true)
                          }
                        />
                      )}
                    </form.Field>
                  </div>
                </div>
                {values.scope === 'user' ? (
                  <label className="grid min-w-0 gap-1 text-xs font-medium sm:max-w-sm">
                    <span>{t('ownerUser')}</span>
                    <Select
                      onValueChange={(value) =>
                        form.setFieldValue('owner_user_id', Number(value))
                      }
                      value={
                        values.owner_user_id == null
                          ? undefined
                          : String(values.owner_user_id)
                      }
                    >
                      <SelectTrigger aria-label={t('ownerUser')}>
                        <SelectValue
                          placeholder={
                            ownerOptionsLoading ? t('loading') : t('ownerUser')
                          }
                        />
                      </SelectTrigger>
                      <SelectContent>
                        {users.map((user) => (
                          <SelectItem
                            key={user.user_id}
                            value={String(user.user_id)}
                          >
                            {user.login_name} — {user.display_name}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </label>
                ) : null}
                <section
                  aria-label={t('target')}
                  className="grid min-w-0 gap-2"
                >
                  <form.Field name="targets" mode="array">
                    {(targetsField) => (
                      <>
                        <div className="flex items-center justify-between gap-2">
                          <h3 className="font-medium">{t('target')}</h3>
                          <Button
                            onClick={() => {
                              const target =
                                createEmptyModelRouteForm().targets[0]
                              if (target) targetsField.pushValue(target)
                            }}
                            size="sm"
                            type="button"
                            variant="outline"
                          >
                            <Plus />
                            {t('addTarget')}
                          </Button>
                        </div>
                        <ol className="grid min-w-0 gap-2">
                          {orderedTargets.map((target, index) => (
                            <li
                              className={`grid min-w-0 gap-2 rounded-lg border p-2 ${drag.dragOverIndex === index ? 'bg-muted' : ''}`}
                              key={targetKey(target)}
                              onDragEnd={drag.onDragEnd}
                              onDragOver={(event) =>
                                drag.onDragOver(event, target)
                              }
                              onDrop={(event) => drag.onDrop(event, target)}
                            >
                              <div className="grid min-w-0 gap-2 md:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)_auto] md:items-center">
                                <div className="flex min-w-0 items-center gap-2">
                                  <button
                                    aria-keyshortcuts="ArrowUp ArrowDown"
                                    aria-label={`${t('target')} ${index + 1}`}
                                    className="inline-flex size-8 shrink-0 cursor-grab items-center justify-center rounded border text-muted-foreground focus-visible:outline-2 focus-visible:outline-primary active:cursor-grabbing"
                                    data-target-drag-handle
                                    draggable
                                    onDragStart={(event) =>
                                      drag.onDragStart(event, target)
                                    }
                                    onKeyDown={(event) =>
                                      drag.onGripKeyDown(event, target)
                                    }
                                    role="button"
                                    tabIndex={0}
                                    type="button"
                                  >
                                    <GripVertical
                                      aria-hidden="true"
                                      className="size-4"
                                    />
                                  </button>
                                  <div className="grid min-w-0 flex-1 gap-1">
                                    <Select
                                      onValueChange={(endpoint_id) =>
                                        updateTarget(index, { endpoint_id })
                                      }
                                      value={target.endpoint_id || undefined}
                                    >
                                      <SelectTrigger
                                        aria-describedby={
                                          target.endpoint_id
                                            ? `${targetKey(target)}-endpoint-value`
                                            : undefined
                                        }
                                        aria-label={`${t('endpoint')} ${index + 1}`}
                                        className="w-full min-w-0"
                                      >
                                        <SelectValue
                                          placeholder={t('endpoint')}
                                        >
                                          <span className="flex min-w-0 items-center gap-2">
                                            {endpointOptionFor(
                                              target.endpoint_id,
                                            ) ? (
                                              <ProviderIcon
                                                provider={
                                                  endpointOptionFor(
                                                    target.endpoint_id,
                                                  )?.provider ?? 'generic'
                                                }
                                                size="xs"
                                              />
                                            ) : null}
                                            <span
                                              className="min-w-0 truncate"
                                              data-target-endpoint-label
                                              id={`${targetKey(target)}-endpoint-value`}
                                            >
                                              {endpointOptionFor(
                                                target.endpoint_id,
                                              )?.label ?? target.endpoint_id}
                                            </span>
                                          </span>
                                        </SelectValue>
                                      </SelectTrigger>
                                      <SelectContent>
                                        {endpointOptions.map((option) => (
                                          <SelectItem
                                            key={option.value}
                                            value={option.value}
                                          >
                                            <span className="flex min-w-0 items-center gap-2">
                                              <ProviderIcon
                                                provider={option.provider}
                                                size="xs"
                                              />
                                              <span className="truncate">
                                                {option.label}
                                              </span>
                                            </span>
                                          </SelectItem>
                                        ))}
                                      </SelectContent>
                                    </Select>
                                  </div>
                                </div>
                                <div className="min-w-0">
                                  <EndpointModelInput
                                    endpointId={target.endpoint_id}
                                    nativeApi={suggestionProtocol(target)}
                                    onChange={(upstream_model) =>
                                      updateTarget(index, { upstream_model })
                                    }
                                    placeholder={t('upstreamModelOptional')}
                                    value={target.upstream_model}
                                  />
                                </div>
                                <div className="flex shrink-0 items-center gap-2">
                                  <Switch
                                    aria-label={`${t('status')} ${t('target')} ${index + 1}`}
                                    checked={target.enabled}
                                    onCheckedChange={(enabled) =>
                                      updateTarget(index, { enabled })
                                    }
                                  />
                                  <Button
                                    aria-label={t('targetSettings')}
                                    aria-pressed={hasTargetSettings(target)}
                                    onClick={() =>
                                      setSettingsTargetIndex(index)
                                    }
                                    size="icon-sm"
                                    title={t('targetSettingsHint')}
                                    type="button"
                                    variant={
                                      hasTargetSettings(target)
                                        ? 'default'
                                        : 'outline'
                                    }
                                  >
                                    <Settings2 />
                                  </Button>
                                  <Button
                                    aria-label={`${t('delete')} ${t('target')} ${index + 1}`}
                                    disabled={orderedTargets.length <= 1}
                                    onClick={() =>
                                      targetsField.removeValue(index)
                                    }
                                    size="icon-sm"
                                    type="button"
                                    variant="ghost"
                                  >
                                    <Trash2 />
                                  </Button>
                                </div>
                              </div>
                              {hasTargetProxy(target) ||
                              hasTargetSchedule(target) ? (
                                <div className="flex min-w-0 flex-wrap gap-x-2 text-xs text-muted-foreground">
                                  {hasTargetProxy(target) ? (
                                    <span>{targetProxySummary(target, t)}</span>
                                  ) : null}
                                  {hasTargetSchedule(target) ? (
                                    <span
                                      title={target.active_windows
                                        .map(
                                          (window) =>
                                            `${window.start}–${window.end}`,
                                        )
                                        .join(', ')}
                                    >
                                      {targetScheduleSummary(target, t)}
                                    </span>
                                  ) : null}
                                </div>
                              ) : null}
                            </li>
                          ))}
                        </ol>
                      </>
                    )}
                  </form.Field>
                </section>
              </>
            )}
          </fieldset>
          <DialogFooter>
            <Button
              disabled={savePending}
              onClick={() =>
                selectedTarget ? backToTargets() : onOpenChange(false)
              }
              type="button"
              variant="outline"
            >
              {t('cancel')}
            </Button>
            <Button disabled={!canSubmit || savePending} type="submit">
              {savePending ? <LoaderCircle className="animate-spin" /> : null}
              {t('save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
