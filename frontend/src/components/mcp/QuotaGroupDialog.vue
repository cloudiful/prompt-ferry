<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { McpQuotaGroup, QuotaGroupRequest } from '@/generated/admin-api'

const props = defineProps<{
  busy: boolean
  header: string
  t: TranslateFn
  group: McpQuotaGroup | null
}>()

const visible = defineModel<boolean>('visible', { required: true })

const emit = defineEmits<{
  save: [request: QuotaGroupRequest]
}>()

const PROVIDER_PRESETS = ['context7', 'firecrawl'] as const
const CUSTOM_PROVIDER = '__custom__'

const name = ref('')
const unit = ref<'requests' | 'credits'>('requests')
const selectedProvider = ref('')
const customProvider = ref('')
const monthlyLimit = ref<number | null>(null)
const dailyLimit = ref<number | null>(null)
const defaultCost = ref(1)
const billingPeriodStart = ref('')
const billingPeriodEnd = ref('')

const providerOptions = computed(() => [
  ...PROVIDER_PRESETS.map((preset) => ({ label: preset, value: preset })),
  { label: props.t('providerCustom'), value: CUSTOM_PROVIDER },
])

function toDateTimeLocal(value: string | null | undefined): string {
  if (!value) return ''
  return value.slice(0, 16)
}

function fromDateTimeLocal(value: string): string | null {
  if (!value) return null
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? null : date.toISOString()
}

function resetForm(): void {
  name.value = ''
  unit.value = 'requests'
  selectedProvider.value = ''
  customProvider.value = ''
  monthlyLimit.value = null
  dailyLimit.value = null
  defaultCost.value = 1
  billingPeriodStart.value = ''
  billingPeriodEnd.value = ''
}

watch(
  () => [visible.value, props.group] as const,
  ([open, group]) => {
    if (!open) return
    resetForm()
    if (!group) return
    name.value = group.name
    unit.value = group.unit === 'credits' ? 'credits' : 'requests'
    const kind = group.provider_kind ?? ''
    if ((PROVIDER_PRESETS as readonly string[]).includes(kind)) {
      selectedProvider.value = kind
    } else {
      selectedProvider.value = kind ? CUSTOM_PROVIDER : ''
      customProvider.value = kind
    }
    monthlyLimit.value = group.monthly_limit ?? null
    dailyLimit.value = group.daily_limit ?? null
    defaultCost.value = group.default_cost
    billingPeriodStart.value = toDateTimeLocal(group.billing_period_start)
    billingPeriodEnd.value = toDateTimeLocal(group.billing_period_end)
  },
)

watch(selectedProvider, (provider) => {
  if (provider === 'context7') unit.value = 'requests'
  if (provider === 'firecrawl') unit.value = 'credits'
})

const canSubmit = computed(() => name.value.trim().length > 0)

function submit(): void {
  if (!canSubmit.value) return
  const providerKind =
    selectedProvider.value === CUSTOM_PROVIDER
      ? customProvider.value.trim() || null
      : selectedProvider.value || null
  emit('save', {
    name: name.value.trim(),
    scope: 'admin',
    provider_kind: providerKind,
    unit,
    monthly_limit: monthlyLimit.value,
    daily_limit: dailyLimit.value,
    default_cost: defaultCost.value,
    billing_period_start: fromDateTimeLocal(billingPeriodStart.value),
    billing_period_end: fromDateTimeLocal(billingPeriodEnd.value),
  })
}
</script>

<template>
  <UModal v-model:open="visible" :title="header">
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="submit">
        <div class="grid gap-2 md:grid-cols-2">
          <label class="grid gap-1">
            <span class="text-muted">{{ t('quotaGroupName') }}</span>
            <UInput v-model="name" :placeholder="t('quotaGroupName')" />
          </label>
          <label class="grid gap-1">
            <span class="text-muted">{{ t('quotaUnit') }}</span>
            <USelect
              v-model="unit"
              :items="[
                { label: t('quotaUnitRequests'), value: 'requests' },
                { label: t('quotaUnitCredits'), value: 'credits' },
              ]"
              label-key="label"
              value-key="value"
            />
          </label>
        </div>
        <label class="grid gap-1">
          <span class="text-muted">{{ t('providerKind') }}</span>
          <USelect
            v-model="selectedProvider"
            :items="providerOptions"
            label-key="label"
            value-key="value"
            :placeholder="t('providerCustom')"
          />
          <UInput
            v-if="selectedProvider === CUSTOM_PROVIDER"
            v-model="customProvider"
            :placeholder="'context7 / firecrawl / …'"
          />
        </label>
        <div class="grid gap-2 md:grid-cols-3">
          <label class="grid gap-1">
            <span class="text-muted">{{ t('monthlyCallLimit') }}</span>
            <UInput
              v-model="monthlyLimit"
              type="number"
              min="0"
              :placeholder="t('quotaUnlimited')"
            />
          </label>
          <label class="grid gap-1">
            <span class="text-muted">{{ t('dailyCallLimit') }}</span>
            <UInput
              v-model="dailyLimit"
              type="number"
              min="0"
              :placeholder="t('quotaUnlimited')"
            />
          </label>
          <label class="grid gap-1">
            <span class="flex items-center gap-1 text-muted">
              <span>{{ t('defaultCost') }}</span>
              <UTooltip :text="t('defaultCostHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('defaultCostHint')"
                />
              </UTooltip>
            </span>
            <UInput v-model="defaultCost" type="number" min="0" step="0.01" />
          </label>
        </div>
        <div class="grid gap-2 md:grid-cols-2">
          <label class="grid gap-1">
            <span class="flex items-center gap-1 text-muted">
              <span>{{ t('billingPeriodStart') }}</span>
              <UTooltip :text="t('billingPeriodHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('billingPeriodHint')"
                />
              </UTooltip>
            </span>
            <UInput v-model="billingPeriodStart" type="datetime-local" />
          </label>
          <label class="grid gap-1">
            <span class="flex items-center gap-1 text-muted">
              <span>{{ t('billingPeriodEnd') }}</span>
              <UTooltip :text="t('billingPeriodHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('billingPeriodHint')"
                />
              </UTooltip>
            </span>
            <UInput v-model="billingPeriodEnd" type="datetime-local" />
          </label>
        </div>
        <div class="flex justify-end gap-2 border-t border-default pt-3">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            @click="visible = false"
            >{{ t('cancel') }}</UButton
          >
          <UButton
            type="submit"
            size="sm"
            :loading="busy"
            :disabled="!canSubmit"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('save')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
