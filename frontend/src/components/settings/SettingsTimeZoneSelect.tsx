import { useEffect, useId, useMemo, useRef, useState } from 'react'
import { Check, ChevronsUpDown, Search } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { cn } from 'cn'
import {
  AUTO_DISPLAY_TIME_ZONE,
  type DisplayTimeZoneOption,
} from '@/format/timezone-core'

/**
 * The popover list for one query.
 *
 * The automatic option always leads the list and the current selection is kept
 * even when the query excludes its text, so filtering never hides or rewrites
 * the stored preference.
 */
function filterZones(
  options: readonly DisplayTimeZoneOption[],
  value: string,
  query: string,
  autoValue: string,
): DisplayTimeZoneOption[] {
  const needle = query.trim().toLowerCase()
  const automatic = options.find(
    (option) => option.value === AUTO_DISPLAY_TIME_ZONE,
  ) ?? {
    label: autoValue,
    value: AUTO_DISPLAY_TIME_ZONE,
  }
  const zones = options.filter((option) => {
    if (option.value === AUTO_DISPLAY_TIME_ZONE) return false
    if (option.value === value) return true
    if (needle.length === 0) return true
    return (
      option.label.toLowerCase().includes(needle) ||
      option.value.toLowerCase().includes(needle)
    )
  })
  return [automatic, ...zones]
}

type SettingsTimeZoneSelectProps = {
  autoValue: string
  filterLabel: string
  id: string
  label: string
  onChange: (zone: string) => void
  options: readonly DisplayTimeZoneOption[]
  value: string
}

/**
 * The searchable display-time-zone picker.
 *
 * The trigger keeps the current selection visible; the popover lists the
 * automatic option first and every zone behind a local filter. Filtering is
 * presentation only: it never rewrites the stored preference, and the current
 * selection stays in the list and selected until the operator actually picks
 * another option. Arrow keys, Home/End, Enter and Escape drive the same
 * selection as touch.
 */
export function SettingsTimeZoneSelect({
  autoValue,
  filterLabel,
  id,
  label,
  onChange,
  options,
  value,
}: SettingsTimeZoneSelectProps) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const filterRef = useRef<HTMLInputElement | null>(null)
  const triggerRef = useRef<HTMLButtonElement | null>(null)
  const listId = useId()

  const selectedLabel =
    options.find((option) => option.value === value)?.label ?? value

  const filtered = useMemo(
    () => filterZones(options, value, query, autoValue),
    [autoValue, options, query, value],
  )

  useEffect(() => {
    if (!open) return
    setQuery('')
    const selectedIndex = options.findIndex((option) => option.value === value)
    setActive(selectedIndex >= 0 ? selectedIndex : 0)
    const frame = requestAnimationFrame(() => filterRef.current?.focus())
    return () => cancelAnimationFrame(frame)
  }, [open, options, value])

  function close(refocus: boolean): void {
    setOpen(false)
    if (refocus) triggerRef.current?.focus()
  }

  function select(option: DisplayTimeZoneOption): void {
    onChange(option.value)
    close(true)
  }

  function move(delta: number): void {
    setActive((current) => {
      const total = filtered.length
      if (total === 0) return 0
      const next = current + delta
      if (next < 0) return total - 1
      if (next >= total) return 0
      return next
    })
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLInputElement>): void {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      move(1)
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      move(-1)
      return
    }
    if (event.key === 'Home') {
      event.preventDefault()
      setActive(0)
      return
    }
    if (event.key === 'End') {
      event.preventDefault()
      setActive(Math.max(0, filtered.length - 1))
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      const option = filtered[active] ?? filtered[0]
      if (option) select(option)
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      close(true)
    }
  }

  return (
    <Popover onOpenChange={setOpen} open={open}>
      <PopoverTrigger asChild>
        <Button
          aria-controls={listId}
          aria-expanded={open}
          aria-haspopup="listbox"
          aria-label={label}
          className="w-full min-w-0 justify-between font-normal"
          id={id}
          ref={triggerRef}
          size="sm"
          type="button"
          variant="outline"
        >
          <span className="min-w-0 truncate" title={selectedLabel}>
            {selectedLabel}
          </span>
          <ChevronsUpDown
            aria-hidden="true"
            className="size-3.5 shrink-0 opacity-60"
          />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-[min(22rem,calc(100vw-2rem))] p-2"
      >
        <div className="relative">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground"
          />
          <Input
            aria-activedescendant={
              filtered[active] ? `${listId}-${active}` : undefined
            }
            aria-autocomplete="list"
            aria-controls={listId}
            aria-expanded={open}
            aria-label={filterLabel}
            className="h-8 pl-7 text-xs"
            onChange={(event) => {
              const nextQuery = event.target.value
              setQuery(nextQuery)
              // A typed query never highlights an option it does not match, so
              // Enter cannot silently keep the automatic option.
              const matches = filterZones(options, value, nextQuery, autoValue)
              const needle = nextQuery.trim().toLowerCase()
              const firstMatch =
                needle.length === 0
                  ? 0
                  : matches.findIndex(
                      (option) =>
                        option.value !== AUTO_DISPLAY_TIME_ZONE &&
                        (option.label.toLowerCase().includes(needle) ||
                          option.value.toLowerCase().includes(needle)),
                    )
              setActive(firstMatch >= 0 ? firstMatch : 0)
            }}
            onKeyDown={handleKeyDown}
            ref={filterRef}
            role="combobox"
            value={query}
          />
        </div>
        <ul
          aria-label={label}
          className="mt-1 grid max-h-64 gap-0.5 overflow-y-auto"
          id={listId}
          role="listbox"
        >
          {filtered.map((option, index) => {
            const selected = option.value === value
            return (
              <li
                aria-selected={selected}
                className={cn(
                  'flex min-h-8 cursor-pointer items-center gap-1.5 rounded px-2 text-xs',
                  index === active
                    ? 'bg-accent text-accent-foreground'
                    : undefined,
                )}
                id={`${listId}-${index}`}
                key={option.value}
                onClick={() => select(option)}
                onMouseDown={(event) => event.preventDefault()}
                onMouseEnter={() => setActive(index)}
                role="option"
                title={option.label}
              >
                <Check
                  aria-hidden="true"
                  className={cn(
                    'size-3.5 shrink-0',
                    selected ? 'opacity-100' : 'opacity-0',
                  )}
                />
                <span className="min-w-0 truncate">{option.label}</span>
              </li>
            )
          })}
        </ul>
      </PopoverContent>
    </Popover>
  )
}
