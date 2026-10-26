import type { StreamDeltaBatchingForm } from '@/models'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Checkbox } from '@/components/ui/checkbox'
import { useLocale } from '@/lib/i18n'
import {
  FLUSH_WINDOW_MS_RANGE,
  MAX_BUFFER_BYTES_RANGE,
  MAX_BUFFER_CHARS_RANGE,
  parseSettingInteger,
} from '@/lib/settings-requests-model'

type StreamDeltaBatchingFieldsProps = {
  disabled?: boolean
  form: StreamDeltaBatchingForm
  onChange: (patch: Partial<StreamDeltaBatchingForm>) => void
}

type IntegerFieldProps = {
  disabled: boolean
  id: string
  label: string
  onChange: (value: number) => void
  range: { min: number; max?: number }
  value: number
}

/**
 * One integer control with the backend's own bounds.
 *
 * The value is a plain integer with no grouping separators, and an edit is
 * clamped to the bounds before it reaches the form. A value loaded from the
 * server outside those bounds stays visible until the operator changes it,
 * because reading a setting is not an edit.
 */
function IntegerField({
  disabled,
  id,
  label,
  onChange,
  range,
  value,
}: IntegerFieldProps) {
  return (
    <div className="grid min-w-0 gap-1">
      <Label className="text-xs text-muted-foreground" htmlFor={id}>
        {label}
      </Label>
      <Input
        className="h-8 text-sm tabular-nums"
        disabled={disabled}
        id={id}
        inputMode="numeric"
        max={range.max}
        min={range.min}
        name={id}
        onChange={(event) => {
          const parsed = parseSettingInteger(event.target.value, range)
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
 * The stream delta batching controls shared by the settings tabs.
 *
 * The switch stays with the owning card header; this component renders the
 * fieldset body, so the same fields can be reused verbatim where the section
 * appears again.
 */
export function StreamDeltaBatchingFields({
  disabled = false,
  form,
  onChange,
}: StreamDeltaBatchingFieldsProps) {
  const { t } = useLocale()

  return (
    <div className="grid min-w-0 gap-3">
      <div className="grid min-w-0 gap-3 md:grid-cols-2">
        <IntegerField
          disabled={disabled}
          id="settings-flush-window-ms"
          label={t('flushWindowMs')}
          onChange={(value) => onChange({ flush_window_ms: value })}
          range={FLUSH_WINDOW_MS_RANGE}
          value={form.flush_window_ms}
        />
        <IntegerField
          disabled={disabled}
          id="settings-max-buffer-chars"
          label={t('maxBufferChars')}
          onChange={(value) => onChange({ max_buffer_chars: value })}
          range={MAX_BUFFER_CHARS_RANGE}
          value={form.max_buffer_chars}
        />
        <IntegerField
          disabled={disabled}
          id="settings-max-buffer-bytes"
          label={t('maxBufferBytes')}
          onChange={(value) => onChange({ max_buffer_bytes: value })}
          range={MAX_BUFFER_BYTES_RANGE}
          value={form.max_buffer_bytes}
        />
        <div className="grid min-w-0 content-end gap-2">
          <Label className="text-xs font-normal text-muted-foreground">
            <Checkbox
              checked={form.flush_on_line_break}
              disabled={disabled}
              id="settings-flush-on-line-break"
              name="settings-flush-on-line-break"
              onCheckedChange={(checked) =>
                onChange({ flush_on_line_break: checked === true })
              }
            />
            {t('flushOnLineBreak')}
          </Label>
          <Label className="text-xs font-normal text-muted-foreground">
            <Checkbox
              checked={form.flush_on_sentence_end}
              disabled={disabled}
              id="settings-flush-on-sentence-end"
              name="settings-flush-on-sentence-end"
              onCheckedChange={(checked) =>
                onChange({ flush_on_sentence_end: checked === true })
              }
            />
            {t('flushOnSentenceEnd')}
          </Label>
        </div>
      </div>
    </div>
  )
}
