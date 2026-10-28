import { useEffect, useId, useRef, useState } from 'react'
import { ChevronsUpDown, X } from 'lucide-react'
import type { NativeApi } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useLocale } from '@/lib/i18n'
import {
  endpointModelCatalogKey,
  loadEndpointModelCatalog,
} from '@/lib/endpoint-models'

type EndpointModelInputProps = {
  disabled?: boolean
  endpointId: string
  nativeApi?: NativeApi
  onChange: (value: string) => void
  placeholder: string
  value: string
}

type CatalogState = {
  error: boolean
  key: string | null
  loading: boolean
  models: string[]
}

const EMPTY_STATE: CatalogState = {
  error: false,
  key: null,
  loading: false,
  models: [],
}

export function EndpointModelInput({
  disabled = false,
  endpointId,
  nativeApi,
  onChange,
  placeholder,
  value,
}: EndpointModelInputProps) {
  const { t } = useLocale()
  const listId = useId()
  const activeKey = endpointId
    ? endpointModelCatalogKey(endpointId, nativeApi)
    : null
  const [catalog, setCatalog] = useState<CatalogState>(EMPTY_STATE)
  const [open, setOpen] = useState(false)
  const [active, setActive] = useState(-1)
  const [hovered, setHovered] = useState(-1)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const restoreFocusRef = useRef(false)
  const current = catalog.key === activeKey
  const models = current ? catalog.models : []
  const failed = current && catalog.error
  const loading = activeKey !== null && (!current || catalog.loading)

  useEffect(() => {
    let currentRequest = true
    if (!endpointId || !activeKey) {
      setCatalog(EMPTY_STATE)
      return () => {
        currentRequest = false
      }
    }
    setCatalog({ error: false, key: activeKey, loading: true, models: [] })
    void loadEndpointModelCatalog(endpointId, nativeApi)
      .then((next) => {
        if (currentRequest)
          setCatalog({
            error: false,
            key: activeKey,
            loading: false,
            models: next,
          })
      })
      .catch(() => {
        if (currentRequest)
          setCatalog({
            error: true,
            key: activeKey,
            loading: false,
            models: [],
          })
      })
    return () => {
      currentRequest = false
    }
  }, [activeKey, endpointId, nativeApi])

  useEffect(() => {
    setOpen(false)
    setActive(-1)
  }, [activeKey])

  function closeList(): void {
    setOpen(false)
    setActive(-1)
    setHovered(-1)
  }

  function closeAndRefocusInput(): void {
    restoreFocusRef.current = true
    setOpen(false)
    setActive(-1)
    setHovered(-1)
  }

  function move(delta: number): void {
    if (models.length === 0) return
    setActive((index) => {
      // Without an active option the first arrow press activates the first or
      // the last option; afterwards the movement clamps and never wraps.
      if (index < 0) return delta > 0 ? 0 : models.length - 1
      return Math.min(models.length - 1, Math.max(0, index + delta))
    })
  }

  function choose(model: string): void {
    onChange(model)
    closeAndRefocusInput()
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLInputElement>): void {
    if (disabled) return
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      if (!open) {
        setOpen(true)
        setActive(-1)
        return
      }
      move(1)
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      if (!open) {
        setOpen(true)
        setActive(-1)
        return
      }
      move(-1)
      return
    }
    if (event.key === 'Home' && open) {
      event.preventDefault()
      if (models.length > 0) setActive(0)
      return
    }
    if (event.key === 'End' && open) {
      event.preventDefault()
      if (models.length > 0) setActive(models.length - 1)
      return
    }
    if (event.key === 'Enter') {
      // A free-text field inside the route form never submits the form; Enter
      // only commits an option the operator deliberately activated.
      event.preventDefault()
      const model = open && active >= 0 ? models[active] : undefined
      if (model !== undefined) choose(model)
      return
    }
    if (event.key === 'Escape' && open) {
      event.preventDefault()
      event.stopPropagation()
      closeAndRefocusInput()
      return
    }
    if (event.key === 'Tab' && open) closeList()
  }

  return (
    <div className="grid min-w-0 gap-1">
      <div className="flex min-w-0 gap-1">
        <Input
          aria-activedescendant={
            open && active >= 0 ? `${listId}-${active}` : undefined
          }
          aria-autocomplete="list"
          aria-controls={listId}
          aria-expanded={open}
          aria-haspopup="listbox"
          aria-label={placeholder}
          autoCapitalize="off"
          autoComplete="off"
          data-model-candidate-input
          disabled={disabled}
          onChange={(event) => onChange(event.currentTarget.value)}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          ref={inputRef}
          role="combobox"
          spellCheck={false}
          value={value}
        />
        {value ? (
          <Button
            aria-label={`${t('delete')} ${t('upstreamModelOptional')}`}
            disabled={disabled}
            onClick={() => onChange('')}
            size="icon"
            type="button"
            variant="outline"
          >
            <X aria-hidden="true" />
          </Button>
        ) : null}
        <Popover
          onOpenChange={(next) => {
            setOpen(next)
            // An open list never starts with an active option, so the first
            // arrow press always activates the first or the last candidate.
            if (next) {
              setActive(-1)
              setHovered(-1)
            }
          }}
          open={open}
        >
          <PopoverTrigger asChild>
            <Button
              aria-controls={listId}
              aria-haspopup="listbox"
              aria-label={t('modelCandidates')}
              disabled={disabled}
              size="icon"
              type="button"
              variant="outline"
            >
              <ChevronsUpDown aria-hidden="true" />
            </Button>
          </PopoverTrigger>
          <PopoverContent
            align="end"
            className="max-h-[min(15rem,40vh)] w-[min(20rem,calc(100vw-2rem))] overflow-y-auto p-1"
            onCloseAutoFocus={(event) => {
              event.preventDefault()
              if (!restoreFocusRef.current) return
              restoreFocusRef.current = false
              inputRef.current?.focus()
            }}
            onFocusOutside={(event) => {
              if (event.target === inputRef.current) event.preventDefault()
            }}
            onOpenAutoFocus={(event) => {
              event.preventDefault()
              inputRef.current?.focus()
            }}
          >
            <ul
              aria-busy={loading}
              aria-label={placeholder}
              className="grid gap-0.5"
              data-model-candidate-list
              id={listId}
              role="listbox"
            >
              {models.map((model, index) => (
                <li
                  aria-selected={index === active}
                  className={`flex min-h-8 cursor-pointer items-center rounded px-2 text-xs ${index === active || (active < 0 && index === hovered) ? 'bg-accent text-accent-foreground' : ''}`}
                  data-model-candidate-option
                  id={`${listId}-${index}`}
                  key={model}
                  onClick={() => choose(model)}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseEnter={() => setHovered(index)}
                  onMouseLeave={() => setHovered(-1)}
                  role="option"
                  title={model}
                >
                  <span className="min-w-0 truncate">{model}</span>
                </li>
              ))}
            </ul>
          </PopoverContent>
        </Popover>
        {loading ? (
          <span aria-label={t('loading')} className="sr-only" role="status">
            {t('loading')}
          </span>
        ) : null}
      </div>
      {failed ? (
        <p className="text-xs text-muted-foreground">
          {t('upstreamModelSuggestionsFailed')}
        </p>
      ) : null}
    </div>
  )
}
