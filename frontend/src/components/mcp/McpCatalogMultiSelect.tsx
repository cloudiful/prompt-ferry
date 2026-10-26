import { useMemo, useState } from 'react'
import { Check, ChevronDown, X } from 'lucide-react'
import type { McpCatalogItem } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useLocale } from '@/lib/i18n'

type McpCatalogMultiSelectProps = {
  disabled?: boolean
  items: McpCatalogItem[]
  loading?: boolean
  onChange: (values: string[]) => void
  placeholder: string
  value: string[]
}

export function McpCatalogMultiSelect({
  disabled = false,
  items,
  loading = false,
  onChange,
  placeholder,
  value,
}: McpCatalogMultiSelectProps) {
  const { t } = useLocale()
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const needsSearch = items.length > 8
  const visibleItems = useMemo(() => {
    if (!needsSearch || search.trim() === '') return items
    const query = search.trim().toLocaleLowerCase()
    return items.filter((item) =>
      `${item.name} ${item.title ?? ''} ${item.description ?? ''}`
        .toLocaleLowerCase()
        .includes(query),
    )
  }, [items, needsSearch, search])

  function changeOpen(next: boolean): void {
    setOpen(next)
    if (!next) setSearch('')
  }

  function toggle(name: string, checked: boolean | 'indeterminate'): void {
    if (checked === true) {
      if (!value.includes(name)) onChange([...value, name])
      return
    }
    onChange(value.filter((item) => item !== name))
  }

  function remove(name: string): void {
    onChange(value.filter((item) => item !== name))
  }

  return (
    <div className="grid min-w-0 gap-2">
      {value.length > 0 ? (
        <ul aria-label={placeholder} className="flex min-w-0 flex-wrap gap-1">
          {value.map((name) => (
            <li
              className="flex min-w-0 max-w-full items-center gap-1 rounded-full border bg-muted px-2 py-1 text-xs"
              key={name}
            >
              <span className="min-w-0 truncate" title={name}>
                {name}
              </span>
              <Button
                aria-label={`${t('delete')} ${name}`}
                className="size-5"
                disabled={disabled || loading}
                onClick={() => remove(name)}
                size="icon-xs"
                type="button"
                variant="ghost"
              >
                <X aria-hidden="true" />
              </Button>
            </li>
          ))}
        </ul>
      ) : null}
      <Popover onOpenChange={changeOpen} open={open}>
        <PopoverTrigger asChild>
          <Button
            aria-label={placeholder}
            className="w-full min-w-0 justify-between"
            disabled={disabled || loading}
            type="button"
            variant="outline"
          >
            <span className="min-w-0 truncate text-left">
              {value.length > 0 ? value.join(', ') : placeholder}
            </span>
            {loading ? (
              <span
                aria-label={t('loading')}
                className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent"
                role="status"
              />
            ) : (
              <ChevronDown aria-hidden="true" />
            )}
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="max-h-[min(60vh,24rem)] w-[min(24rem,calc(100vw-2rem))] overflow-y-auto p-2"
        >
          {needsSearch ? (
            <Input
              aria-label={`${t('filter')} ${placeholder}`}
              autoComplete="off"
              className="mb-2"
              onChange={(event) => setSearch(event.currentTarget.value)}
              placeholder={t('filter')}
              value={search}
            />
          ) : null}
          {visibleItems.length > 0 ? (
            <ul className="grid gap-1">
              {visibleItems.map((item) => {
                const selected = value.includes(item.name)
                return (
                  <li key={item.name}>
                    <label
                      className="flex min-w-0 cursor-pointer items-start gap-2 rounded-md px-2 py-2 text-sm hover:bg-accent"
                      title={item.description ?? item.title ?? item.name}
                    >
                      <Checkbox
                        checked={selected}
                        onCheckedChange={(checked) =>
                          toggle(item.name, checked)
                        }
                      />
                      <span className="grid min-w-0 flex-1 gap-0.5">
                        <span className="flex min-w-0 items-center gap-1">
                          <span className="min-w-0 flex-1 truncate font-mono">
                            {item.name}
                          </span>
                          {selected ? (
                            <Check aria-hidden="true" className="size-3.5" />
                          ) : null}
                        </span>
                        {item.title || item.description ? (
                          <span className="line-clamp-2 break-words text-xs text-muted-foreground">
                            {item.title ?? item.description}
                          </span>
                        ) : null}
                      </span>
                    </label>
                  </li>
                )
              })}
            </ul>
          ) : (
            <p className="px-2 py-3 text-sm text-muted-foreground">
              {t('noData')}
            </p>
          )}
        </PopoverContent>
      </Popover>
    </div>
  )
}
