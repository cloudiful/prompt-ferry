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
  <section class="grid gap-3 rounded-lg bg-default p-3">
    <div class="flex flex-wrap items-center gap-2">
      <span class="text-sm font-semibold text-highlighted">{{
        t('billingPeriod')
      }}</span>
      <UInput
        :model-value="startDate"
        type="date"
        size="sm"
        @update:model-value="emit('period', String($event ?? ''), endDate)"
      />
      <span class="text-xs text-muted">to</span>
      <UInput
        :model-value="endDate"
        type="date"
        size="sm"
        @update:model-value="emit('period', startDate, String($event ?? ''))"
      />
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        icon="i-lucide-filter"
        @click="apply"
      >
        {{ t('filterBilling') }}
      </UButton>
    </div>
    <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
      <USelect
        v-if="isAdmin"
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
        @update:model-value="updateUser"
      />
      <UInput
        :model-value="filters.client_key_id"
        type="number"
        :placeholder="t('billingClientKeyId')"
        @update:model-value="
          filters.client_key_id = Number($event) || undefined
        "
      />
      <UInput
        :model-value="filters.requested_model"
        :placeholder="t('publicModel')"
        @update:model-value="
          filters.requested_model = String($event ?? '') || undefined
        "
      />
      <USelect
        v-if="isAdmin"
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
        @update:model-value="filters.endpoint_id = $event || undefined"
      />
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
        @update:model-value="filters.usage_status = $event || undefined"
      />
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
        @update:model-value="filters.pricing_status = $event || undefined"
      />
    </div>
  </section>
</template>
