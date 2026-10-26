import { Plus, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useLocale, type MessageValues } from '@/lib/i18n'
import type { MessageKey } from '@/i18n'
import type {} from '@/types/i18n'
import { scheduleRowError } from '@/models/endpoints/schedule-windows'

export type ScheduleWindow = { start: string; end: string; days?: number[] }

type ScheduleWindowsFieldsProps = {
  onChange: (windows: ScheduleWindow[], touched: boolean) => void
  windows: ScheduleWindow[]
}

const ALL_DAYS = [1, 2, 3, 4, 5, 6, 7]

export function ScheduleWindowsFields({
  onChange,
  windows,
}: ScheduleWindowsFieldsProps) {
  const { t } = useLocale()

  function translateForModel(key: MessageKey, ...args: unknown[]): string {
    return t(key, args[0] as MessageValues | undefined)
  }

  const rowErrors = windows.map((row) =>
    scheduleRowError(row, translateForModel),
  )

  function updateRow(index: number, patch: Partial<ScheduleWindow>): void {
    onChange(
      windows.map((row, rowIndex) =>
        rowIndex === index ? { ...row, ...patch } : row,
      ),
      true,
    )
  }

  function rowDays(index: number): number[] {
    const days = windows[index]?.days
    return !days || days.length === 0 ? ALL_DAYS : days
  }

  function setRowDays(index: number, days: number[]): void {
    updateRow(index, { days: [...days] })
  }

  function toggleRowDay(index: number, day: number): void {
    const current = new Set(rowDays(index))
    if (current.has(day)) current.delete(day)
    else current.add(day)
    setRowDays(
      index,
      [...current].sort((a, b) => a - b),
    )
  }

  return (
    <div className="grid gap-3">
      {windows.map((row, index) => (
        <div className="grid min-w-0 gap-2 rounded-md border p-3" key={index}>
          <div className="grid min-w-0 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] items-end gap-2">
            <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
              <span>{t('scheduleStart')}</span>
              <Input
                onChange={(event) =>
                  updateRow(index, { start: event.currentTarget.value })
                }
                type="time"
                value={row.start}
              />
            </label>
            <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
              <span>{t('scheduleEnd')}</span>
              <Input
                inputMode="numeric"
                maxLength={5}
                onChange={(event) =>
                  updateRow(index, { end: event.currentTarget.value })
                }
                pattern="([01][0-9]|2[0-3]):[0-5][0-9]|24:00"
                placeholder="HH:MM"
                type="text"
                value={row.end}
              />
            </label>
            <Button
              aria-label={t('delete')}
              onClick={() =>
                onChange(
                  windows.filter((_, rowIndex) => rowIndex !== index),
                  true,
                )
              }
              size="icon-sm"
              type="button"
              variant="ghost"
            >
              <Trash2 aria-hidden="true" />
            </Button>
          </div>
          {rowErrors[index] ? (
            <p className="text-xs text-destructive">{rowErrors[index]}</p>
          ) : null}
          <div className="flex flex-wrap gap-1">
            <Button
              onClick={() => setRowDays(index, [1, 2, 3, 4, 5])}
              size="xs"
              type="button"
              variant="ghost"
            >
              {t('weekdays')}
            </Button>
            <Button
              onClick={() => setRowDays(index, [6, 7])}
              size="xs"
              type="button"
              variant="ghost"
            >
              {t('weekend')}
            </Button>
            <Button
              onClick={() => setRowDays(index, ALL_DAYS)}
              size="xs"
              type="button"
              variant="ghost"
            >
              {t('everyDay')}
            </Button>
            {ALL_DAYS.map((day) => (
              <Button
                aria-pressed={rowDays(index).includes(day)}
                key={day}
                onClick={() => toggleRowDay(index, day)}
                size="xs"
                type="button"
                variant={rowDays(index).includes(day) ? 'secondary' : 'outline'}
              >
                {t(`weekday${day}` as MessageKey)}
              </Button>
            ))}
          </div>
        </div>
      ))}
      <Button
        className="justify-self-start"
        onClick={() => onChange([...windows, { start: '', end: '' }], true)}
        size="sm"
        type="button"
        variant="outline"
      >
        <Plus aria-hidden="true" />
        {t('scheduleAddWindow')}
      </Button>
    </div>
  )
}
