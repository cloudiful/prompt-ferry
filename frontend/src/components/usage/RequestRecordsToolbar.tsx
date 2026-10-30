import { useEffect, useRef, useState } from 'react'
import { Search } from 'lucide-react'
import type { RequestRecordFacets } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  createUsageFacetOptionsView,
  createUsageStateOptions,
} from '@/models/usage'
import { useLocale } from '@/lib/i18n'
import type { UsageCategory, UsageFilters, UsageScope } from '@/lib/usage-model'

/** The internal "no filter" value; it never reaches the store filters. */
const ANY_VALUE = '__any__'

type FilterOption = { label: string; value: string }

type FilterSelectProps = {
  ariaLabel: string
  name: string
  onChange: (value: string | null) => void
  options: FilterOption[]
  placeholder: string
  value: string | null
}

function FilterSelect({
  ariaLabel,
  name,
  onChange,
  options,
  placeholder,
  value,
}: FilterSelectProps) {
  return (
    <Select
      onValueChange={(next) => onChange(next === ANY_VALUE ? null : next)}
      value={value ?? ANY_VALUE}
    >
      <SelectTrigger
        aria-label={ariaLabel}
        className="w-40"
        data-usage-filter={name}
        size="sm"
      >
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ANY_VALUE}>{placeholder}</SelectItem>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

type RequestRecordsToolbarProps = {
  category: UsageCategory
  facets: RequestRecordFacets
  filters: UsageFilters
  isAdmin: boolean
  onFilters: (patch: Partial<UsageFilters>) => void
  onScope: (scope: UsageScope) => void
  onSearch: (search: string | null) => void
  scope: UsageScope
}

/**
 * The records toolbar: a draft search that commits on Enter, immediate filter
 * selects fed by the current facets and the MCP scope.
 *
 * Every action is a callback; the toolbar issues no read itself and never
 * persists anything. Select values are mapped back to a real `null` filter, so
 * the internal "any" sentinel cannot leak into the request query.
 */
export function RequestRecordsToolbar({
  category,
  facets,
  filters,
  isAdmin,
  onFilters,
  onScope,
  onSearch,
  scope,
}: RequestRecordsToolbarProps) {
  const { t } = useLocale()
  const [search, setSearch] = useState(filters.search ?? '')
  const committed = useRef(filters.search ?? '')

  useEffect(() => {
    setSearch(filters.search ?? '')
    committed.current = filters.search ?? ''
  }, [filters.search])

  const stateOptions = createUsageStateOptions(
    {
      aborted: t('requestStateAborted'),
      awaiting_approval: t('requestStateAwaitingApproval'),
      completed: t('requestStateCompleted'),
      failed: t('requestStateFailed'),
      received: t('requestStateReceived'),
      upstream_processing: t('requestStateUpstreamProcessing'),
    },
    facets.states ?? [],
  )
  const facetOptions = createUsageFacetOptionsView(facets, stateOptions)

  function commitSearch(): void {
    const next = search.trim()
    if (next === committed.current) return
    committed.current = next
    onSearch(next === '' ? null : next)
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="relative min-w-64 flex-1">
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <Input
          className="pl-8"
          data-usage-toolbar-search
          onChange={(event) => setSearch(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return
            event.preventDefault()
            commitSearch()
          }}
          placeholder={t('search')}
          value={search}
        />
      </div>
      <FilterSelect
        ariaLabel={t('allClientKeys')}
        name="client_key_id"
        onChange={(value) =>
          onFilters({
            client_key_id: value === null ? null : Number(value),
          })
        }
        options={facetOptions.request_client_key_options.map((option) => ({
          label: option.label,
          value: String(option.value),
        }))}
        placeholder={t('allClientKeys')}
        value={
          filters.client_key_id === null ? null : String(filters.client_key_id)
        }
      />
      {isAdmin && (
        <FilterSelect
          ariaLabel={t('allUsers')}
          name="user"
          onChange={(value) => onFilters({ user: value })}
          options={facetOptions.request_user_options.map((option) => ({
            label: option.label,
            value: String(option.value),
          }))}
          placeholder={t('allUsers')}
          value={filters.user}
        />
      )}
      <FilterSelect
        ariaLabel={category === 'ai' ? t('allModels') : t('mcpServer')}
        name="model"
        onChange={(value) => onFilters({ model: value })}
        options={facetOptions.request_model_options.map((option) => ({
          label: option.label,
          value: String(option.value),
        }))}
        placeholder={category === 'ai' ? t('allModels') : t('mcpServer')}
        value={filters.model}
      />
      {category === 'mcp' && (
        <Select
          onValueChange={(value) => onScope(value as UsageScope)}
          value={scope}
        >
          <SelectTrigger
            aria-label={t('mcpScope')}
            className="w-40"
            data-usage-filter="scope"
            size="sm"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="service">{t('mcpScopeService')}</SelectItem>
            <SelectItem value="gateway">{t('mcpScopeGateway')}</SelectItem>
            <SelectItem value="all">{t('mcpScopeAll')}</SelectItem>
          </SelectContent>
        </Select>
      )}
      <FilterSelect
        ariaLabel={t('allStatus')}
        name="request_state"
        onChange={(value) => onFilters({ request_state: value })}
        options={facetOptions.request_state_options.map((option) => ({
          label: option.label,
          value: String(option.value),
        }))}
        placeholder={t('allStatus')}
        value={filters.request_state}
      />
      {category === 'ai' && (
        <FilterSelect
          ariaLabel={t('allRedactionStates')}
          name="redaction_applied"
          onChange={(value) =>
            onFilters({
              redaction_applied: value === null ? null : value === 'true',
            })
          }
          options={facetOptions.request_redaction_options.map((option) => ({
            label: option.label,
            value: String(option.value),
          }))}
          placeholder={t('allRedactionStates')}
          value={
            filters.redaction_applied === null
              ? null
              : String(filters.redaction_applied)
          }
        />
      )}
    </div>
  )
}
