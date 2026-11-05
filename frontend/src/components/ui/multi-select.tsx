'use client'

import * as React from 'react'
import { ChevronDown } from 'lucide-react'
import { cn } from 'cn'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'

export type MultiSelectOption = {
  value: string
  label: string
}

export type MultiSelectProps = {
  allLabel: string
  className?: string
  disabled?: boolean
  label: string
  limitLabel?: string
  maxSelected?: number
  onChange: (next: string[]) => void
  options: readonly MultiSelectOption[]
  selected: readonly string[]
  summary: string
}

/**
 * Compact checkbox multi-select over an explicit option list. An empty
 * selection is the implicit "all" state and is represented only by the
 * distinct All option; individual options are checked only when explicitly
 * selected. Removing the final explicit choice returns to the All state.
 */
export function MultiSelect({
  allLabel,
  className,
  disabled,
  label,
  limitLabel,
  maxSelected,
  onChange,
  options,
  selected,
  summary,
}: MultiSelectProps) {
  const id = React.useId()
  const selectedSet = new Set(selected)
  const allSelected = selected.length === 0
  const atLimit = maxSelected !== undefined && selected.length >= maxSelected

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          aria-label={label}
          className={cn(
            'h-9 min-w-40 max-w-full justify-between font-normal',
            className,
          )}
          disabled={disabled}
          type="button"
          variant="outline"
        >
          <span className="min-w-0 truncate text-left">{summary}</span>
          <ChevronDown
            aria-hidden="true"
            className="size-4 shrink-0 opacity-50"
          />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-64 p-1">
        <div aria-label={label} className="grid gap-0.5" role="group">
          <label
            className="flex cursor-pointer items-center gap-2 rounded-sm px-2 py-1.5 text-sm hover:bg-accent"
            htmlFor={`${id}-all`}
          >
            <Checkbox
              checked={allSelected}
              id={`${id}-all`}
              onCheckedChange={() => onChange([])}
            />
            <span className="min-w-0 break-words">{allLabel}</span>
          </label>
          <div className="my-1 h-px bg-border" />
          <div className="max-h-64 overflow-y-auto">
            {options.map((option) => {
              const checked = selectedSet.has(option.value)
              const optionDisabled =
                atLimit && !checked && maxSelected !== undefined
              return (
                <label
                  className={cn(
                    'flex items-center gap-2 rounded-sm px-2 py-1.5 text-sm',
                    optionDisabled
                      ? 'cursor-not-allowed opacity-50'
                      : 'cursor-pointer hover:bg-accent',
                  )}
                  htmlFor={`${id}-${option.value}`}
                  key={option.value}
                >
                  <Checkbox
                    checked={checked}
                    disabled={optionDisabled}
                    id={`${id}-${option.value}`}
                    onCheckedChange={() => {
                      if (optionDisabled) return
                      const next = checked
                        ? selected.filter((current) => current !== option.value)
                        : [...selected, option.value]
                      onChange(next)
                    }}
                  />
                  <span className="min-w-0 break-words">{option.label}</span>
                </label>
              )
            })}
          </div>
          {atLimit && limitLabel ? (
            <p
              className="px-2 py-1 text-xs text-amber-700 dark:text-amber-300"
              role="status"
            >
              {limitLabel}
            </p>
          ) : null}
        </div>
      </PopoverContent>
    </Popover>
  )
}
