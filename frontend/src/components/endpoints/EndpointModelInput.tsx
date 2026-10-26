import { useEffect, useId, useState } from 'react'
import { X } from 'lucide-react'
import type { NativeApi } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
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

  return (
    <div className="grid min-w-0 gap-1">
      <div className="flex min-w-0 gap-1">
        <Input
          aria-label={placeholder}
          autoComplete="off"
          disabled={disabled}
          list={listId}
          onChange={(event) => onChange(event.currentTarget.value)}
          placeholder={placeholder}
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
        {loading ? (
          <span aria-label={t('loading')} className="sr-only" role="status">
            {t('loading')}
          </span>
        ) : null}
      </div>
      <datalist id={listId}>
        {models.map((model) => (
          <option key={model} value={model} />
        ))}
      </datalist>
      {failed ? (
        <p className="text-xs text-muted-foreground">
          {t('upstreamModelSuggestionsFailed')}
        </p>
      ) : null}
    </div>
  )
}
