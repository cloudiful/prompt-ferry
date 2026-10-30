import { useEffect, useId, useMemo, useRef, useState } from 'react'
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
  const query = value.trim().toLocaleLowerCase()
  const matching = useMemo(
    () =>
      query === ''
        ? models
        : models.filter((model) => model.toLocaleLowerCase().includes(query)),
    [models, query],
  )
  // A filter, catalog, endpoint or protocol change invalidates whatever the
  // operator had highlighted before; the option list is never longer than the
  // current match set, so aria-activedescendant always names a real option.
  const activeIndex = active >= 0 && active < matching.length ? active : -1
  const empty = !loading && !failed && query !== '' && matching.length === 0

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

  useEffect(() => {
    setActive(-1)
    setHovered(-1)
  }, [activeKey, catalog.models, query])

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

  function openList(): void {
    setOpen(true)
    setActive(-1)
    setHovered(-1)
  }

  function handleValueChange(next: string): void {
    onChange(next)
    // Editing the text shows the matches for what the operator just typed;
    // nothing is highlighted until an explicit arrow press.
    if (endpointId) openList()
  }

  function move(delta: number): void {
    if (matching.length === 0) return
    setActive((index) => {
      // Without an active option the first arrow press activates the first or
      // the last match; afterwards the movement clamps and never wraps.
      if (index < 0 || index >= matching.length)
        return delta > 0 ? 0 : matching.length - 1
      return Math.min(matching.length - 1, Math.max(0, index + delta))
    })
  }

  function choose(model: string): void {
    onChange(model)
    closeAndRefocusInput()
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLInputElement>): void {
    if (disabled) return
    // A composing IME owns Enter, Escape and the arrow keys until the
    // composition is committed, so the candidate list never selects or
    // submits mid-composition.
    if (event.nativeEvent.isComposing || event.keyCode === 229) return
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      if (!open) {
        openList()
        return
      }
      move(1)
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      if (!open) {
        openList()
        return
      }
      move(-1)
      return
    }
    if (event.key === 'Home' && open) {
      event.preventDefault()
      if (matching.length > 0) setActive(0)
      return
    }
    if (event.key === 'End' && open) {
      event.preventDefault()
      if (matching.length > 0) setActive(matching.length - 1)
      return
    }
    if (event.key === 'Enter') {
      // A free-text field inside the route form never submits the form; Enter
      // only commits a match the operator deliberately activated.
      event.preventDefault()
      const model = open && activeIndex >= 0 ? matching[activeIndex] : undefined
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
            open && activeIndex >= 0 ? `${listId}-${activeIndex}` : undefined
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
          onChange={(event) => handleValueChange(event.currentTarget.value)}
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
            onClick={() => {
              onChange('')
              closeList()
            }}
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
            // arrow press always activates the first or the last match.
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
              {matching.map((model, index) => (
                <li
                  aria-selected={index === activeIndex}
                  className={`flex min-h-8 cursor-pointer items-center rounded px-2 text-xs ${index === activeIndex || (activeIndex < 0 && index === hovered) ? 'bg-accent text-accent-foreground' : ''}`}
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
            {empty ? (
              <p
                className="px-2 py-3 text-xs text-muted-foreground"
                data-model-candidate-empty
                role="status"
              >
                {t('noData')}
              </p>
            ) : null}
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
