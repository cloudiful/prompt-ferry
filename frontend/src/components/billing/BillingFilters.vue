<script setup lang="ts">
import type { User } from '@/generated/admin-api'
import type { ProviderEndpoint } from '@/generated/admin-api'
import type { BillingChargeFilters } from '@/models/billing'

const props = defineProps<{
  filters: BillingChargeFilters
  endpoints: ProviderEndpoint[]
  isAdmin: boolean
  startDate: string
  endDate: string
  users: User[]
  t: TranslateFn
}>()

const emit = defineEmits<{
  apply: [filters: BillingChargeFilters]
  period: [start: string, end: string]
}>()

function apply(): void {
  emit('apply', { ...props.filters })
}

function updateUser(value: unknown): void {
  const userId = Number(value)
  emit('apply', {
    ...props.filters,
    user_id: Number.isFinite(userId) ? userId : undefined,
  })
}
</script>

<template>
  <section class="grid min-w-0 gap-3 rounded-lg bg-default p-3">
    <h2 class="text-sm font-semibold text-highlighted">
      {{ t('billingPeriod') }}
    </h2>
    <div
      class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-end gap-2 sm:flex sm:flex-wrap"
    >
      <UFormField
        :label="t('billingPeriodStart')"
        name="billing_period_start"
        class="min-w-0"
      >
        <UInput
          :model-value="startDate"
          type="date"
          size="sm"
          class="w-full sm:w-40"
          @update:model-value="emit('period', String($event ?? ''), endDate)"
        />
      </UFormField>
      <span class="pb-2 text-xs text-muted">{{ t('billingPeriodTo') }}</span>
      <UFormField
        :label="t('billingPeriodEnd')"
        name="billing_period_end"
        class="min-w-0"
      >
        <UInput
          :model-value="endDate"
          type="date"
          size="sm"
          class="w-full sm:w-40"
          @update:model-value="emit('period', startDate, String($event ?? ''))"
        />
      </UFormField>
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        icon="i-lucide-filter"
        class="col-span-3 w-full sm:w-auto"
        @click="apply"
      >
        {{ t('filterBilling') }}
      </UButton>
    </div>
    <div class="grid min-w-0 gap-2 sm:grid-cols-2 lg:grid-cols-4">
      <UFormField
        v-if="isAdmin"
        :label="t('billingFilterUser')"
        name="billing_filter_user"
        class="min-w-0"
      >
        <USelect
          :model-value="filters.user_id"
          :items="[
            { label: t('allUsers'), value: undefined },
            ...users.map((user) => ({
              label: user.login_name,
              value: user.user_id,
            })),
          ]"
          label-key="label"
          value-key="value"
          :placeholder="t('allUsers')"
          class="w-full"
          @update:model-value="updateUser"
        />
      </UFormField>
      <UFormField
        :label="t('billingClientKeyId')"
        name="billing_filter_client_key"
        class="min-w-0"
      >
        <UInput
          :model-value="filters.client_key_id"
          type="number"
          :placeholder="t('billingClientKeyId')"
          class="w-full"
          @update:model-value="
            filters.client_key_id = Number($event) || undefined
          "
        />
      </UFormField>
      <UFormField
        :label="t('publicModel')"
        name="billing_filter_requested_model"
        class="min-w-0"
      >
        <UInput
          :model-value="filters.requested_model"
          :placeholder="t('publicModel')"
          class="w-full"
          @update:model-value="
            filters.requested_model = String($event ?? '') || undefined
          "
        />
      </UFormField>
      <UFormField
        v-if="isAdmin"
        :label="t('billingFilterEndpoint')"
        name="billing_filter_endpoint"
        class="min-w-0"
      >
        <USelect
          :model-value="filters.endpoint_id"
          :items="[
            { label: t('allEndpoints'), value: undefined },
            ...endpoints.map((endpoint) => ({
              label: endpoint.name,
              value: endpoint.endpoint_id,
            })),
          ]"
          label-key="label"
          value-key="value"
          :placeholder="t('allEndpoints')"
          class="w-full"
          @update:model-value="filters.endpoint_id = $event || undefined"
        />
      </UFormField>
      <UFormField
        :label="t('billingFilterUsage')"
        name="billing_filter_usage_status"
        class="min-w-0"
      >
        <USelect
          :model-value="filters.usage_status"
          :items="[
            { label: t('allUsageStates'), value: undefined },
            { label: t('usageKnown'), value: 'known' },
            { label: t('usageUnknown'), value: 'unknown' },
          ]"
          label-key="label"
          value-key="value"
          :placeholder="t('allUsageStates')"
          class="w-full"
          @update:model-value="filters.usage_status = $event || undefined"
        />
      </UFormField>
      <UFormField
        :label="t('billingFilterPricing')"
        name="billing_filter_pricing_status"
        class="min-w-0"
      >
        <USelect
          :model-value="filters.pricing_status"
          :items="[
            { label: t('allBillingStates'), value: undefined },
            { label: t('pricingPriced'), value: 'priced' },
            { label: t('pricingUnpriced'), value: 'unpriced' },
          ]"
          label-key="label"
          value-key="value"
          :placeholder="t('allBillingStates')"
          class="w-full"
          @update:model-value="filters.pricing_status = $event || undefined"
        />
      </UFormField>
    </div>
  </section>
</template>
