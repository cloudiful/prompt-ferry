import { Filter } from 'lucide-react'
import type { ProviderEndpoint, User } from '@/generated/admin-api'
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
import type { BillingChargeFilters } from '@/lib/billing-model'

const ALL = 'all'

type BillingFiltersProps = {
  endpoints: ProviderEndpoint[]
  filters: BillingChargeFilters
  isAdmin: boolean
  periodEnd: string
  periodStart: string
  users: User[]
  onApply: () => void
  onChange: (filters: BillingChargeFilters) => void
  onPeriod: (start: string, end: string) => void
}

export function BillingFilters({
  endpoints,
  filters,
  isAdmin,
  periodEnd,
  periodStart,
  users,
  onApply,
  onChange,
  onPeriod,
}: BillingFiltersProps) {
  const { t } = useLocale()

  return (
    <section className="grid min-w-0 gap-3 rounded-lg border p-3">
      <h2 className="m-0 text-sm font-semibold">{t('billingPeriod')}</h2>
      <div className="grid min-w-0 grid-cols-1 items-end gap-2 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto]">
        <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
          <span>{t('billingPeriodStart')}</span>
          <Input
            aria-label={t('billingPeriodStart')}
            name="billing_period_start"
            onChange={(event) => onPeriod(event.currentTarget.value, periodEnd)}
            type="date"
            value={periodStart}
          />
        </label>
        <span className="hidden pb-2 text-xs text-muted-foreground sm:block">
          {t('billingPeriodTo')}
        </span>
        <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
          <span>{t('billingPeriodEnd')}</span>
          <Input
            aria-label={t('billingPeriodEnd')}
            name="billing_period_end"
            onChange={(event) =>
              onPeriod(periodStart, event.currentTarget.value)
            }
            type="date"
            value={periodEnd}
          />
        </label>
        <Button onClick={onApply} size="sm" type="button" variant="outline">
          <Filter aria-hidden="true" />
          {t('filterBilling')}
        </Button>
      </div>
      <div className="grid min-w-0 gap-2 sm:grid-cols-2 lg:grid-cols-4">
        {isAdmin && (
          <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
            <span>{t('billingFilterUser')}</span>
            <Select
              onValueChange={(value) =>
                onChange({
                  ...filters,
                  user_id: value === ALL ? undefined : Number(value),
                })
              }
              value={
                filters.user_id === undefined ? ALL : String(filters.user_id)
              }
            >
              <SelectTrigger
                aria-label={t('billingFilterUser')}
                className="w-full"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>{t('allUsers')}</SelectItem>
                {users.map((user) => (
                  <SelectItem key={user.user_id} value={String(user.user_id)}>
                    {user.login_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
        )}
        <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
          <span>{t('billingClientKeyId')}</span>
          <Input
            aria-label={t('billingClientKeyId')}
            inputMode="numeric"
            name="billing_filter_client_key"
            onChange={(event) =>
              onChange({
                ...filters,
                client_key_id: Number(event.currentTarget.value) || undefined,
              })
            }
            type="number"
            value={filters.client_key_id ?? ''}
          />
        </label>
        <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
          <span>{t('publicModel')}</span>
          <Input
            aria-label={t('publicModel')}
            name="billing_filter_requested_model"
            onChange={(event) =>
              onChange({
                ...filters,
                requested_model: event.currentTarget.value || undefined,
              })
            }
            type="text"
            value={filters.requested_model ?? ''}
          />
        </label>
        {isAdmin && (
          <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
            <span>{t('billingFilterEndpoint')}</span>
            <Select
              onValueChange={(value) =>
                onChange({
                  ...filters,
                  endpoint_id: value === ALL ? undefined : value,
                })
              }
              value={filters.endpoint_id ?? ALL}
            >
              <SelectTrigger
                aria-label={t('billingFilterEndpoint')}
                className="w-full"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ALL}>{t('allEndpoints')}</SelectItem>
                {endpoints.map((endpoint) => (
                  <SelectItem
                    key={endpoint.endpoint_id}
                    value={endpoint.endpoint_id}
                  >
                    {endpoint.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
        )}
        <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
          <span>{t('billingFilterUsage')}</span>
          <Select
            onValueChange={(value) =>
              onChange({
                ...filters,
                usage_status: value === ALL ? undefined : value,
              })
            }
            value={filters.usage_status ?? ALL}
          >
            <SelectTrigger
              aria-label={t('billingFilterUsage')}
              className="w-full"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>{t('allUsageStates')}</SelectItem>
              <SelectItem value="known">{t('usageKnown')}</SelectItem>
              <SelectItem value="unknown">{t('usageUnknown')}</SelectItem>
            </SelectContent>
          </Select>
        </label>
        <label className="grid min-w-0 gap-1 text-xs text-muted-foreground">
          <span>{t('billingFilterPricing')}</span>
          <Select
            onValueChange={(value) =>
              onChange({
                ...filters,
                pricing_status: value === ALL ? undefined : value,
              })
            }
            value={filters.pricing_status ?? ALL}
          >
            <SelectTrigger
              aria-label={t('billingFilterPricing')}
              className="w-full"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={ALL}>{t('allBillingStates')}</SelectItem>
              <SelectItem value="priced">{t('pricingPriced')}</SelectItem>
              <SelectItem value="unpriced">{t('pricingUnpriced')}</SelectItem>
            </SelectContent>
          </Select>
        </label>
      </div>
    </section>
  )
}
