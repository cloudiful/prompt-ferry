import { useEffect, useState } from 'react'
import type { NativeApi } from '@/generated/admin-api'
import type { ModelRouteTargetForm } from '@/models'
import { parseEmbeddingDimensionsInput } from '@/admin-mappers/forms/model-route'
import { ProxySettingsFields } from '@/components/shared/ProxySettingsFields'
import { ScheduleWindowsFields } from '@/components/shared/ScheduleWindowsFields'
import { ServiceTierOverrideField } from '@/components/shared/ServiceTierOverrideField'
import { SettingsFieldRow } from '@/components/shared/SettingsFieldRow'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import type { Translate } from '@/lib/i18n'
import {
  effectiveTargetProtocol,
  normalizeNativeApiAxis,
  supportsChatAdaptationFor,
  supportsEmbeddingDimensionOverrideFor,
  supportsServiceTierFor,
} from '@/models/endpoints/service-tier'
import { isTargetProxyValid } from '@/models/endpoints/model-route-target'

const NATIVE_APIS: NativeApi[] = [
  'auto',
  'chat',
  'embeddings',
  'responses',
  'anthropic_messages',
  'realtime',
]

const THINKING_EFFORTS = [
  'inherit',
  'none',
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
]
const COMPACT_MODES = ['passthrough', 'self_summarize', 'off']

type ModelRouteTargetSettingsPageProps = {
  endpointProtocol?: NativeApi | null
  onChange: (target: ModelRouteTargetForm) => void
  target: ModelRouteTargetForm
}

export function ModelRouteTargetSettingsPage({
  endpointProtocol,
  onChange,
  target,
}: ModelRouteTargetSettingsPageProps) {
  const { t } = useLocale()
  const [dimensionText, setDimensionText] = useState('')
  const effectiveProtocol = effectiveTargetProtocol(
    target.native_api,
    normalizeNativeApiAxis(endpointProtocol),
  )
  const chatAdaptationEligible = supportsChatAdaptationFor(effectiveProtocol)
  const serviceTierEligible = supportsServiceTierFor(effectiveProtocol)
  const dimensionEligible =
    supportsEmbeddingDimensionOverrideFor(effectiveProtocol)

  useEffect(() => {
    setDimensionText(
      target.embedding_dimensions_override == null
        ? ''
        : String(target.embedding_dimensions_override),
    )
    if (target.embedding_dimensions_invalid)
      onChange({ ...target, embedding_dimensions_invalid: false })
  }, [
    target.embedding_dimensions_override,
    target.endpoint_id,
    target.native_api,
  ])

  function update(patch: Partial<ModelRouteTargetForm>): void {
    onChange({ ...target, ...patch })
  }

  function changeDimensions(raw: string): void {
    setDimensionText(raw)
    const parsed = parseEmbeddingDimensionsInput(raw)
    update({
      embedding_dimensions_invalid: parsed.invalid,
      ...(parsed.invalid
        ? {}
        : { embedding_dimensions_override: parsed.value }),
    })
  }

  return (
    <div className="grid gap-3 rounded-md border bg-muted p-3">
      <SettingsFieldRow
        hint={t('modelRouteNativeApiHint')}
        label={t('modelRouteNativeApi')}
      >
        <Select
          value={target.native_api ?? 'auto'}
          onValueChange={(value) =>
            update({ native_api: normalizeNativeApiAxis(value) })
          }
        >
          <SelectTrigger
            aria-label={t('modelRouteNativeApi')}
            className="w-full"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {NATIVE_APIS.map((api) => (
              <SelectItem key={api} value={api}>
                {nativeApiLabel(api, t)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingsFieldRow>
      {dimensionEligible ? (
        <SettingsFieldRow
          hint={t('embeddingDimensionsOverrideHint')}
          label={t('embeddingDimensionsOverride')}
        >
          <div className="grid gap-1">
            <input
              aria-describedby={
                target.embedding_dimensions_invalid
                  ? 'target-embedding-dimensions-error'
                  : undefined
              }
              aria-label={t('embeddingDimensionsOverride')}
              aria-invalid={target.embedding_dimensions_invalid === true}
              className="h-9 w-full rounded-md border bg-background px-3 text-sm"
              inputMode="numeric"
              onChange={(event) => changeDimensions(event.currentTarget.value)}
              placeholder={t('embeddingDimensionsInherit')}
              value={dimensionText}
            />
            {target.embedding_dimensions_invalid ? (
              <p
                className="text-xs text-destructive"
                id="target-embedding-dimensions-error"
              >
                {t('embeddingDimensionsInvalid')}
              </p>
            ) : null}
          </div>
        </SettingsFieldRow>
      ) : null}
      {chatAdaptationEligible && serviceTierEligible ? (
        <ServiceTierOverrideField
          onChange={(value) => update({ service_tier: value })}
          value={target.service_tier ?? null}
        />
      ) : null}
      <SettingsFieldRow
        hint={t('proxyUrlOverrideHint')}
        label={t('proxyUrlOverride')}
      >
        <div className="grid gap-1">
          <ProxySettingsFields
            hasSaved={target.has_saved_proxy_url_override}
            onChange={({ proxyUrl, hasSaved }) =>
              update({
                proxy_url_override: proxyUrl,
                has_saved_proxy_url_override: hasSaved,
              })
            }
            proxyUrl={target.proxy_url_override}
          />
          {!isTargetProxyValid(target) ? (
            <p className="text-xs text-destructive" role="alert">
              {t('proxyUrlOverrideHint')}
            </p>
          ) : null}
        </div>
      </SettingsFieldRow>
      <SettingsFieldRow
        hint={t('scheduleWindowsHint')}
        label={t('scheduleWindows')}
      >
        <ScheduleWindowsFields
          onChange={(active_windows, active_windows_touched) =>
            update({ active_windows, active_windows_touched })
          }
          windows={target.active_windows}
        />
      </SettingsFieldRow>
      {chatAdaptationEligible ? (
        <>
          <SettingsFieldRow
            hint={t('thinkingEffortOverrideHint')}
            label={t('thinkingEffortOverride')}
          >
            <Select
              value={target.thinking_effort_override ?? 'inherit'}
              onValueChange={(value) =>
                update({
                  thinking_effort_override: value === 'inherit' ? null : value,
                })
              }
            >
              <SelectTrigger
                aria-label={t('thinkingEffortOverride')}
                className="w-full"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {THINKING_EFFORTS.map((effort) => (
                  <SelectItem key={effort} value={effort}>
                    {effort === 'inherit' ? t('thinkingEffortInherit') : effort}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingsFieldRow>
          <SettingsFieldRow
            hint={t('compactModeHint')}
            label={t('compactMode')}
          >
            <Select
              value={target.compact_mode || 'passthrough'}
              onValueChange={(value) => update({ compact_mode: value })}
            >
              <SelectTrigger aria-label={t('compactMode')} className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {COMPACT_MODES.map((mode) => (
                  <SelectItem key={mode} value={mode}>
                    {compactModeLabel(mode, t)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingsFieldRow>
          <SettingsFieldRow
            hint={t('normalizeHint')}
            label={t('normalizeLabel')}
          >
            <div className="flex justify-end">
              <Switch
                aria-label={t('normalizeLabel')}
                checked={target.dev_system_normalize}
                onCheckedChange={(dev_system_normalize) =>
                  update({ dev_system_normalize })
                }
              />
            </div>
          </SettingsFieldRow>
          <SettingsFieldRow
            hint={t('thinkingDowngradeHint')}
            label={t('thinkingDowngradeLabel')}
          >
            <div className="flex justify-end">
              <Switch
                aria-label={t('thinkingDowngradeLabel')}
                checked={target.thinking_downgrade_enabled}
                onCheckedChange={(thinking_downgrade_enabled) =>
                  update({ thinking_downgrade_enabled })
                }
              />
            </div>
          </SettingsFieldRow>
        </>
      ) : null}
    </div>
  )
}

function nativeApiLabel(api: NativeApi, t: Translate): string {
  const keys = {
    auto: 'endpointSourceAuto',
    chat: 'nativeApiChat',
    embeddings: 'nativeApiEmbeddings',
    responses: 'nativeApiResponses',
    anthropic_messages: 'nativeApiAnthropicMessages',
    realtime: 'nativeApiRealtime',
  } as const
  return t(keys[api])
}

function compactModeLabel(mode: string, t: Translate): string {
  const key =
    mode === 'passthrough'
      ? 'compactModePassthrough'
      : mode === 'self_summarize'
        ? 'compactModeSelfSummarize'
        : 'compactModeOff'
  return t(key)
}
