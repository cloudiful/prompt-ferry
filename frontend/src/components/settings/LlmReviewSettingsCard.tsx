import { ChevronDown } from 'lucide-react'
import type { ReactNode } from 'react'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useLocale } from '@/lib/i18n'
import {
  FAILURE_POLICY_OPTIONS,
  PENDING_TTL_RANGE,
  REVIEW_TIMEOUT_RANGE,
  parseLlmReviewHeaders,
  type LlmReviewField,
  type LlmReviewForm,
} from '@/lib/settings-review-model'
import { parseSettingInteger } from '@/lib/settings-requests-model'
import type { ReviewFailurePolicy } from '@/generated/admin-api'

type LlmReviewSettingsCardProps = {
  disabled: boolean
  form: LlmReviewForm
  onChange: <K extends LlmReviewField>(
    field: K,
    value: LlmReviewForm[K],
  ) => void
}

/**
 * One collapsible group, as a native disclosure.
 *
 * The browser owns the open state, the keyboard behaviour and the accessible
 * name, so the group needs no primitive of its own and the three groups stay
 * independent of each other.
 */
function FieldGroup({
  children,
  label,
  open,
}: {
  children: ReactNode
  label: string
  open?: boolean
}) {
  return (
    <details
      className="group min-w-0 rounded-md border border-border/60 px-3 py-2"
      open={open}
    >
      <summary className="flex cursor-pointer list-none items-center justify-between gap-2 text-xs font-medium marker:hidden">
        <span>{label}</span>
        <ChevronDown
          aria-hidden="true"
          className="size-3.5 text-muted-foreground transition-transform group-open:rotate-180"
        />
      </summary>
      <div className="grid min-w-0 gap-3 pt-2.5">{children}</div>
    </details>
  )
}

function TextField({
  disabled,
  id,
  label,
  onChange,
  placeholder,
  type,
  value,
}: {
  disabled: boolean
  id: string
  label: string
  onChange: (value: string) => void
  placeholder?: string
  type?: 'password' | 'text'
  value: string
}) {
  return (
    <div className="grid min-w-0 gap-1">
      <Label className="text-xs font-medium" htmlFor={id}>
        {label}
      </Label>
      <Input
        autoComplete="off"
        className="h-8 text-sm"
        disabled={disabled}
        id={id}
        name={id}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        type={type ?? 'text'}
        value={value}
      />
    </div>
  )
}

function IntegerField({
  disabled,
  id,
  label,
  minimum,
  onChange,
  value,
}: {
  disabled: boolean
  id: string
  label: string
  minimum: number
  onChange: (value: number) => void
  value: number
}) {
  return (
    <div className="grid min-w-0 gap-1">
      <Label className="text-xs font-medium" htmlFor={id}>
        {label}
      </Label>
      <Input
        className="h-8 text-sm tabular-nums"
        disabled={disabled}
        id={id}
        inputMode="numeric"
        min={minimum}
        name={id}
        onChange={(event) => {
          const parsed = parseSettingInteger(event.target.value, {
            min: minimum,
          })
          if (parsed === null) return
          onChange(parsed)
        }}
        step={1}
        type="number"
        value={value}
      />
    </div>
  )
}

/**
 * The LLM review fields.
 *
 * The two secrets are password inputs and nothing else on this surface ever
 * renders their value: not a title, a tooltip, a placeholder or a message. The
 * stored value is echoed by the server, so an untouched save sends it back and an
 * emptied field clears it.
 */
export function LlmReviewSettingsCard({
  disabled,
  form,
  onChange,
}: LlmReviewSettingsCardProps) {
  const { t } = useLocale()
  const headersMalformed = !parseLlmReviewHeaders(form.webhook_headers_text).ok

  return (
    <div className="grid min-w-0 gap-3">
      <div className="grid min-w-0 gap-3 sm:grid-cols-2">
        <TextField
          disabled={disabled}
          id="settings-llm-review-base-url"
          label={t('reviewBaseUrl')}
          onChange={(value) => onChange('review_base_url', value)}
          placeholder={t('reviewBaseUrlPlaceholder')}
          value={form.review_base_url}
        />
        <TextField
          disabled={disabled}
          id="settings-llm-review-model"
          label={t('reviewModel')}
          onChange={(value) => onChange('review_model', value)}
          placeholder={t('reviewModelPlaceholder')}
          value={form.review_model}
        />
      </div>

      <FieldGroup label={t('reviewExecution')} open>
        <div className="grid min-w-0 gap-3 sm:grid-cols-2">
          <TextField
            disabled={disabled}
            id="settings-llm-review-api-key"
            label={t('apiKey')}
            onChange={(value) => onChange('review_api_key', value)}
            type="password"
            value={form.review_api_key}
          />
          <div className="grid min-w-0 gap-1">
            <Label
              className="text-xs font-medium"
              htmlFor="settings-llm-review-failure-policy"
            >
              {t('failurePolicy')}
            </Label>
            <Select
              onValueChange={(value) =>
                onChange('failure_policy', value as ReviewFailurePolicy)
              }
              value={form.failure_policy}
            >
              <SelectTrigger
                aria-label={t('failurePolicy')}
                className="w-full min-w-0"
                disabled={disabled}
                id="settings-llm-review-failure-policy"
                name="settings-llm-review-failure-policy"
                size="sm"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {FAILURE_POLICY_OPTIONS.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {t(option.labelKey)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <IntegerField
            disabled={disabled}
            id="settings-llm-review-timeout-ms"
            label={t('reviewTimeoutMs')}
            minimum={REVIEW_TIMEOUT_RANGE.min}
            onChange={(value) => onChange('review_timeout_ms', value)}
            value={form.review_timeout_ms}
          />
          <IntegerField
            disabled={disabled}
            id="settings-llm-review-pending-ttl-seconds"
            label={t('pendingTtlSeconds')}
            minimum={PENDING_TTL_RANGE.min}
            onChange={(value) => onChange('pending_ttl_seconds', value)}
            value={form.pending_ttl_seconds}
          />
        </div>
      </FieldGroup>

      <FieldGroup label={t('customPolicyText')}>
        <div className="grid min-w-0 gap-1">
          <Label
            className="text-xs font-medium"
            htmlFor="settings-llm-review-custom-policy"
          >
            {t('customPolicyText')}
          </Label>
          <Textarea
            className="min-h-20 resize-y text-sm"
            disabled={disabled}
            id="settings-llm-review-custom-policy"
            name="settings-llm-review-custom-policy"
            onChange={(event) =>
              onChange('custom_policy_text', event.target.value)
            }
            rows={3}
            value={form.custom_policy_text}
          />
        </div>
      </FieldGroup>

      <FieldGroup label={t('webhook')}>
        <div className="flex min-w-0 items-center gap-2">
          <Switch
            aria-label={t('webhook')}
            checked={form.webhook_enabled}
            disabled={disabled}
            id="settings-llm-review-webhook-enabled"
            name="settings-llm-review-webhook-enabled"
            onCheckedChange={(checked) =>
              onChange('webhook_enabled', checked === true)
            }
          />
          <Label
            className="text-xs font-medium"
            htmlFor="settings-llm-review-webhook-enabled"
          >
            {t('webhook')}
          </Label>
        </div>
        <div className="grid min-w-0 gap-3 md:grid-cols-2">
          <TextField
            disabled={disabled}
            id="settings-llm-review-webhook-url"
            label={t('webhookUrl')}
            onChange={(value) => onChange('webhook_url', value)}
            value={form.webhook_url}
          />
          <TextField
            disabled={disabled}
            id="settings-llm-review-webhook-bearer-token"
            label={t('webhookBearerToken')}
            onChange={(value) => onChange('webhook_bearer_token', value)}
            type="password"
            value={form.webhook_bearer_token}
          />
        </div>
        <div className="grid min-w-0 gap-1">
          <Label
            className="text-xs font-medium"
            htmlFor="settings-llm-review-extra-headers"
          >
            {t('extraHeaders')}
          </Label>
          <Textarea
            aria-invalid={headersMalformed}
            className="min-h-20 resize-y font-mono text-xs"
            disabled={disabled}
            id="settings-llm-review-extra-headers"
            name="settings-llm-review-extra-headers"
            onChange={(event) =>
              onChange('webhook_headers_text', event.target.value)
            }
            placeholder={t('extraHeadersPlaceholder')}
            rows={3}
            spellCheck={false}
            value={form.webhook_headers_text}
          />
          <p className="m-0 text-[11px] leading-relaxed text-muted-foreground">
            {headersMalformed
              ? t('invalidReviewHeadersFormat')
              : t('extraHeadersHint')}
          </p>
        </div>
      </FieldGroup>
    </div>
  )
}
