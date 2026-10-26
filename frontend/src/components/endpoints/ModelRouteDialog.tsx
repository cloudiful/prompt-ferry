import { useRef, useState } from 'react'
import {
  ArrowUp,
  GripVertical,
  LoaderCircle,
  Plus,
  Settings2,
  Trash2,
} from 'lucide-react'
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
  form: ModelRouteForm
  onChange: (form: ModelRouteForm) => void
  onOpenChange: (open: boolean) => void
  onSave: () => void
  open: boolean
  ownerOptionsLoading: boolean
  savePending: boolean
  users: User[]
}

export function ModelRouteDialog({
  endpoints,
  endpointOptions,
  form,
  onChange,
  onOpenChange,
  onSave,
  open,
  ownerOptionsLoading,
  savePending,
  users,
}: ModelRouteDialogProps) {
  const { t } = useLocale()
  const [settingsTarget, setSettingsTarget] =
    useState<ModelRouteTargetForm | null>(null)
  const targetKeys = useRef(new WeakMap<ModelRouteTargetForm, string>())
  const nextTargetKey = useRef(0)
  const orderedTargets = form.targets
  const drag = useModelRouteTargetDrag(orderedTargets, (targets) =>
    patchForm({ targets }),
  )
  const selectedTarget = settingsTarget
    ? (orderedTargets.find((target) => target === settingsTarget) ?? null)
    : null
  const canSubmit =
    form.model_pattern.trim() !== '' &&
    form.targets.length > 0 &&
    form.targets.every((target) => target.endpoint_id.trim() !== '') &&
    (form.scope !== 'user' || form.owner_user_id !== null) &&
    canSaveTargets(form.targets)

  function targetKey(target: ModelRouteTargetForm): string {
    const current = targetKeys.current.get(target)
    if (current) return current
    const next = `route-target-${++nextTargetKey.current}`
    targetKeys.current.set(target, next)
    return next
  }

  function patchForm(patch: Partial<ModelRouteForm>): void {
    onChange({ ...form, ...patch })
  }

  function patchTarget(
    target: ModelRouteTargetForm,
    value: ModelRouteTargetForm,
  ): void {
    const current = form.targets.find((item) => item === target)
    if (!current) return
    Object.assign(current, value)
    patchForm({ targets: [...form.targets] })
  }

  function updateTarget(
    target: ModelRouteTargetForm,
    patch: Partial<ModelRouteTargetForm>,
  ): void {
    const current = form.targets.find((item) => item === target)
    if (!current) return
    Object.assign(current, patch)
    patchForm({ targets: [...form.targets] })
  }

  function addTarget(): void {
    const target = createEmptyModelRouteForm().targets[0]
    if (!target) return
    patchForm({ targets: [...form.targets, target] })
  }

  function removeTarget(target: ModelRouteTargetForm): void {
    if (form.targets.length <= 1) return
    patchForm({ targets: form.targets.filter((item) => item !== target) })
    if (settingsTarget === target) setSettingsTarget(null)
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

  function openSettings(target: ModelRouteTargetForm): void {
    setSettingsTarget(target)
  }

  function backToTargets(): void {
    setSettingsTarget(null)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[min(94vh,58rem)] max-w-5xl overflow-y-auto"
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
            {t(form.rule_id ? 'editModelRoute' : 'newModelRoute')}
          </DialogTitle>
        </DialogHeader>
        {selectedTarget ? (
          <fieldset className="grid gap-3" disabled={savePending}>
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
              endpointProtocol={endpointProtocolFor(selectedTarget.endpoint_id)}
              onChange={(value) => patchTarget(selectedTarget, value)}
              target={selectedTarget}
            />
          </fieldset>
        ) : (
          <form
            className="grid min-w-0 gap-4"
            onSubmit={(event) => {
              event.preventDefault()
              if (canSubmit && !savePending) onSave()
            }}
          >
            <fieldset className="grid min-w-0 gap-4" disabled={savePending}>
              <div className="grid min-w-0 gap-3 sm:grid-cols-[9rem_minmax(0,1fr)_auto] sm:items-end">
                <label className="grid gap-1 text-xs font-medium">
                  <span>{t('scope')}</span>
                  <Select
                    onValueChange={(value) =>
                      patchForm({
                        scope: value === 'user' ? 'user' : 'admin',
                        owner_user_id:
                          value === 'user' ? form.owner_user_id : null,
                      })
                    }
                    value={form.scope}
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
                  <Input
                    aria-label={t('modelPattern')}
                    className="min-w-0"
                    onChange={(event) =>
                      patchForm({ model_pattern: event.currentTarget.value })
                    }
                    value={form.model_pattern}
                  />
                </label>
                <div className="flex items-center gap-2 pb-2">
                  <span className="text-xs font-medium">{t('status')}</span>
                  <Switch
                    aria-label={t('status')}
                    checked={form.enabled}
                    onCheckedChange={(enabled) => patchForm({ enabled })}
                  />
                </div>
              </div>
              {form.scope === 'user' ? (
                <label className="grid min-w-0 gap-1 text-xs font-medium sm:max-w-sm">
                  <span>{t('ownerUser')}</span>
                  <Select
                    onValueChange={(value) =>
                      patchForm({ owner_user_id: Number(value) })
                    }
                    value={
                      form.owner_user_id == null
                        ? undefined
                        : String(form.owner_user_id)
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
              <section className="grid min-w-0 gap-2" aria-label={t('target')}>
                <div className="flex items-center justify-between gap-2">
                  <h3 className="font-medium">{t('target')}</h3>
                  <Button
                    onClick={addTarget}
                    size="sm"
                    type="button"
                    variant="outline"
                  >
                    <Plus />
                    {t('addTarget')}
                  </Button>
                </div>
                <ol className="grid min-w-0 gap-2">
                  {form.targets.map((target, index) => (
                    <li
                      className={`grid min-w-0 gap-2 rounded-lg border p-2 ${drag.dragOverIndex === index ? 'bg-muted' : ''}`}
                      key={targetKey(target)}
                      onDragEnd={drag.onDragEnd}
                      onDragOver={(event) => drag.onDragOver(event, target)}
                      onDrop={(event) => drag.onDrop(event, target)}
                    >
                      <div className="grid min-w-0 gap-2 md:grid-cols-[minmax(0,1.15fr)_minmax(0,1fr)_auto_auto] md:items-center">
                        <div className="flex min-w-0 items-start gap-2">
                          <button
                            aria-keyshortcuts="ArrowUp ArrowDown"
                            aria-label={`${t('target')} ${index + 1}`}
                            className="mt-1 inline-flex size-8 shrink-0 cursor-grab items-center justify-center rounded border text-muted-foreground focus-visible:outline-2 focus-visible:outline-primary active:cursor-grabbing"
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
                          <label className="grid min-w-0 flex-1 gap-1 text-xs font-medium">
                            <span>{t('endpoint')}</span>
                            <Select
                              onValueChange={(endpoint_id) =>
                                updateTarget(target, { endpoint_id })
                              }
                              value={target.endpoint_id || undefined}
                            >
                              <SelectTrigger
                                aria-label={`${t('endpoint')} ${index + 1}`}
                                className="min-w-0"
                              >
                                <SelectValue placeholder={t('endpoint')} />
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
                          </label>
                        </div>
                        <div className="min-w-0">
                          <EndpointModelInput
                            endpointId={target.endpoint_id}
                            nativeApi={suggestionProtocol(target)}
                            onChange={(upstream_model) =>
                              updateTarget(target, { upstream_model })
                            }
                            placeholder={t('upstreamModelOptional')}
                            value={target.upstream_model}
                          />
                        </div>
                        <div className="flex min-w-0 items-center gap-2">
                          <Switch
                            aria-label={`${t('status')} ${t('target')} ${index + 1}`}
                            checked={target.enabled}
                            onCheckedChange={(enabled) =>
                              updateTarget(target, { enabled })
                            }
                          />
                          <Button
                            aria-label={t('targetSettings')}
                            aria-pressed={hasTargetSettings(target)}
                            onClick={() => openSettings(target)}
                            size="icon-sm"
                            title={t('targetSettingsHint')}
                            type="button"
                            variant={
                              hasTargetSettings(target) ? 'default' : 'outline'
                            }
                          >
                            <Settings2 />
                          </Button>
                          <Button
                            aria-label={`${t('delete')} ${t('target')} ${index + 1}`}
                            disabled={form.targets.length <= 1}
                            onClick={() => removeTarget(target)}
                            size="icon-sm"
                            type="button"
                            variant="ghost"
                          >
                            <Trash2 />
                          </Button>
                        </div>
                      </div>
                      <div className="flex min-w-0 flex-wrap gap-x-2 text-xs text-muted-foreground">
                        <span>{targetProxySummary(target, t)}</span>
                        {hasTargetSchedule(target) ? (
                          <span
                            title={target.active_windows
                              .map((window) => `${window.start}–${window.end}`)
                              .join(', ')}
                          >
                            {targetScheduleSummary(target, t)}
                          </span>
                        ) : null}
                      </div>
                    </li>
                  ))}
                </ol>
              </section>
            </fieldset>
            <DialogFooter>
              <Button
                disabled={savePending}
                onClick={() => onOpenChange(false)}
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
        )}
        {selectedTarget ? (
          <DialogFooter>
            <Button
              disabled={savePending}
              onClick={backToTargets}
              type="button"
              variant="outline"
            >
              {t('cancel')}
            </Button>
            <Button
              disabled={!canSubmit || savePending}
              onClick={onSave}
              type="button"
            >
              {t('save')}
            </Button>
          </DialogFooter>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
