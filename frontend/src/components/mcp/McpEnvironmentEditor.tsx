import { Eye, EyeOff, Plus, Trash2 } from 'lucide-react'
import { useState } from 'react'
import type { McpEnvironmentVariableForm } from '@/models'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useLocale } from '@/lib/i18n'

type McpEnvironmentEditorProps = {
  disabled?: boolean
  onChange: (variables: McpEnvironmentVariableForm[]) => void
  variables: McpEnvironmentVariableForm[]
}

export function McpEnvironmentEditor({
  disabled = false,
  onChange,
  variables,
}: McpEnvironmentEditorProps) {
  const { t } = useLocale()
  const [visibleValues, setVisibleValues] = useState<Record<number, boolean>>(
    {},
  )

  function update(
    index: number,
    patch: Partial<McpEnvironmentVariableForm>,
  ): void {
    onChange(
      variables.map((variable, row) =>
        row === index ? { ...variable, ...patch } : variable,
      ),
    )
  }

  function add(): void {
    onChange([
      ...variables,
      { name: '', source: 'worker', value: '', has_saved_value: false },
    ])
  }

  function remove(index: number): void {
    onChange(variables.filter((_, row) => row !== index))
    setVisibleValues((current) => {
      const next: Record<number, boolean> = {}
      for (const [key, visible] of Object.entries(current)) {
        const row = Number(key)
        if (row === index) continue
        next[row > index ? row - 1 : row] = visible
      }
      return next
    })
  }

  function changeSource(index: number, source: 'worker' | 'value'): void {
    const variable = variables[index]
    if (!variable || variable.source === source) return
    update(index, { source, value: '', has_saved_value: false })
  }

  function toggleValueVisibility(index: number): void {
    setVisibleValues((current) => ({
      ...current,
      [index]: current[index] !== true,
    }))
  }

  return (
    <section aria-label={t('stdioEnv')} className="grid min-w-0 gap-2">
      <div className="flex min-w-0 flex-wrap items-start justify-between gap-2">
        <div className="grid min-w-0 gap-1">
          <h3 className="text-xs font-medium">{t('stdioEnv')}</h3>
          <p className="text-xs text-muted-foreground">{t('stdioEnvHint')}</p>
        </div>
        <Button
          disabled={disabled}
          onClick={add}
          size="sm"
          type="button"
          variant="outline"
        >
          <Plus aria-hidden="true" />
          {t('addEnvironmentVariable')}
        </Button>
      </div>
      <div className="grid min-w-0 gap-2">
        {variables.map((variable, index) => (
          <div
            className="grid min-w-0 gap-2 rounded-md border p-2 md:grid-cols-[minmax(0,1fr)_minmax(9rem,0.8fr)_minmax(0,1fr)_auto] md:items-center"
            key={`environment-${index}`}
          >
            <Input
              aria-label={`${t('environmentVariableName')} ${index + 1}`}
              autoComplete="off"
              disabled={disabled}
              onChange={(event) =>
                update(index, { name: event.currentTarget.value })
              }
              placeholder={t('environmentVariableNamePlaceholder')}
              value={variable.name}
            />
            <Select
              onValueChange={(source) =>
                changeSource(index, source as 'worker' | 'value')
              }
              value={variable.source}
            >
              <SelectTrigger
                aria-label={`${t('environmentVariableSource')} ${index + 1}`}
                className="w-full"
                disabled={disabled}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="worker">
                  {t('environmentSourceWorker')}
                </SelectItem>
                <SelectItem value="value">
                  {t('environmentSourceValue')}
                </SelectItem>
              </SelectContent>
            </Select>
            <div className="flex min-w-0 items-center gap-1">
              <Input
                aria-label={`${t('environmentVariableValue')} ${index + 1}`}
                autoComplete="off"
                disabled={disabled}
                onChange={(event) =>
                  update(index, { value: event.currentTarget.value })
                }
                placeholder={
                  variable.source === 'worker'
                    ? t('environmentWorkerNamePlaceholder')
                    : variable.has_saved_value
                      ? t('savedSecret')
                      : t('environmentValuePlaceholder')
                }
                type={
                  variable.source === 'value' && visibleValues[index] !== true
                    ? 'password'
                    : 'text'
                }
                value={variable.value}
              />
              {variable.source === 'value' ? (
                <Button
                  aria-label={
                    visibleValues[index] ? t('hideSecret') : t('showSecret')
                  }
                  disabled={disabled}
                  onClick={() => toggleValueVisibility(index)}
                  size="icon-sm"
                  type="button"
                  variant="ghost"
                >
                  {visibleValues[index] ? (
                    <EyeOff aria-hidden="true" />
                  ) : (
                    <Eye aria-hidden="true" />
                  )}
                </Button>
              ) : null}
            </div>
            <Button
              aria-label={`${t('delete')} ${t('environmentVariableName')} ${index + 1}`}
              disabled={disabled}
              onClick={() => remove(index)}
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              <Trash2 aria-hidden="true" />
            </Button>
          </div>
        ))}
      </div>
    </section>
  )
}
