<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import PageIntro from '@/components/PageIntro.vue'
import BillingBreakdownPanel from '@/components/billing/BillingBreakdownPanel.vue'
import BillingChargeDetailDialog from '@/components/billing/BillingChargeDetailDialog.vue'
import BillingChargesPanel from '@/components/billing/BillingChargesPanel.vue'
import BillingFilters from '@/components/billing/BillingFilters.vue'
import BillingPriceRuleDialog from '@/components/billing/BillingPriceRuleDialog.vue'
import BillingPriceRulesPanel from '@/components/billing/BillingPriceRulesPanel.vue'
import BillingSummaryPanel from '@/components/billing/BillingSummaryPanel.vue'
import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import { useUsersStore } from '@/stores/users'
import { useSessionStore } from '@/stores/session'
import { useBillingStore } from '@/stores/billing'
import {
  currentMonthDate,
  dateInputValue,
  newBillingPriceRuleForm,
  type BillingChargeFilters,
  type BillingPriceRuleForm,
} from '@/models/billing'

const { t } = useLocale()
const { notifyApiError, notifySuccess } = useNotifier()
const route = useRoute()
const session = useSessionStore()
const users = useUsersStore()
const billing = useBillingStore()

const periodStart = ref(currentMonthDate())
const periodEnd = ref('')
const priceRuleVisible = ref(false)
const priceRuleForm = ref<BillingPriceRuleForm>(newBillingPriceRuleForm())
const detailVisible = ref(false)

async function refresh(): Promise<void> {
  try {
    await billing.refresh(session.isAdmin)
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function applyPeriod(start: string, end: string): Promise<void> {
  periodStart.value = start || currentMonthDate()
  periodEnd.value = end
  try {
    await billing.applyPeriod(periodStart.value, periodEnd.value)
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function applyFilters(filters: BillingChargeFilters): Promise<void> {
  try {
    await billing.applyFilters(filters)
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function openDetail(chargeId: number): Promise<void> {
  try {
    await billing.openDetail(chargeId)
    detailVisible.value = true
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function savePriceRule(): Promise<void> {
  const form = priceRuleForm.value
  try {
    await billing.savePriceRule(
      {
        price_side: form.price_side,
        public_model:
          form.price_side === 'sale' ? form.public_model.trim() : null,
        endpoint_id: form.price_side === 'cost' ? form.endpoint_id : null,
        upstream_model:
          form.price_side === 'cost' ? form.upstream_model.trim() : null,
        input_rate: form.input_rate.trim(),
        cache_read_rate: form.cache_read_rate.trim(),
        cache_write_rate: form.cache_write_rate.trim(),
        output_rate: form.output_rate.trim(),
        effective_from: new Date(form.effective_from).toISOString(),
      },
      form.price_rule_id,
    )
    priceRuleVisible.value = false
    notifySuccess(
      form.price_rule_id ? t('priceRuleUpdated') : t('priceRuleCreated'),
    )
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function togglePriceRule(
  rule: (typeof billing.priceRules)[number],
): Promise<void> {
  try {
    await billing.togglePriceRule(rule)
    notifySuccess(rule.enabled ? t('priceRuleDisabled') : t('priceRuleEnabled'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function reprice(): Promise<void> {
  try {
    const count = await billing.reprice()
    notifySuccess(t('repriced', { count }))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function download(kind: 'details' | 'monthly'): Promise<void> {
  try {
    await billing.downloadCsv(kind, `prompt-ferry-billing-${kind}.csv`)
    notifySuccess(t('billingCsvDownloaded'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

function openPriceRule(rule?: (typeof billing.priceRules)[number]): void {
  priceRuleForm.value = newBillingPriceRuleForm(rule)
  priceRuleVisible.value = true
}

async function deletePriceRule(
  rule: (typeof billing.priceRules)[number],
): Promise<void> {
  if (!window.confirm(t('priceRuleDeleteConfirm'))) return
  try {
    await billing.removePriceRule(rule.price_rule_id)
    notifySuccess(t('priceRuleDeleted'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(async () => {
  const requestId =
    typeof route.query.request_id === 'string'
      ? route.query.request_id
      : undefined
  billing.configureFilters({
    ...billing.filters,
    request_id: requestId,
  })
  billing.configurePeriod(periodStart.value, periodEnd.value)
  try {
    if (session.isAdmin) await users.loadUsers()
    await refresh()
  } catch (cause) {
    notifyApiError(cause)
  }
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro
      :eyebrow="t('observability')"
      :title="t('billing')"
      :subtitle="t('billingSubtitle')"
    >
      <template #actions>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          icon="i-lucide-refresh-cw"
          :loading="billing.loading"
          :aria-label="t('billingRefresh')"
          @click="refresh"
          >{{ t('billingRefresh') }}</UButton
        >
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          icon="i-lucide-download"
          @click="download('details')"
          >{{ t('billingExportDetails') }}</UButton
        >
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          icon="i-lucide-calendar-arrow-down"
          @click="download('monthly')"
          >{{ t('billingExportMonthly') }}</UButton
        >
        <UButton
          v-if="session.isAdmin"
          size="sm"
          color="warning"
          variant="outline"
          icon="i-lucide-calculator"
          @click="reprice"
          >{{ t('reprice') }}</UButton
        >
      </template>
    </PageIntro>

    <BillingFilters
      :filters="billing.filters"
      :endpoints="billing.endpoints"
      :is-admin="session.isAdmin"
      :start-date="dateInputValue(periodStart)"
      :end-date="periodEnd"
      :users="users.users"
      :t="t"
      @apply="applyFilters"
      @period="applyPeriod"
    />
    <BillingSummaryPanel
      :summary="billing.summary"
      :is-admin="session.isAdmin"
      :t="t"
    />
    <div class="grid gap-3 xl:grid-cols-2">
      <BillingBreakdownPanel
        :rows="billing.summary?.by_client_key ?? []"
        :is-admin="session.isAdmin"
        :title="t('billingClientKeys')"
        :t="t"
      />
      <BillingBreakdownPanel
        :rows="billing.summary?.by_model ?? []"
        :is-admin="session.isAdmin"
        :title="t('billingModels')"
        :t="t"
      />
    </div>
    <BillingChargesPanel
      :charges="billing.charges"
      :first="billing.first"
      :is-admin="session.isAdmin"
      :loading="billing.chargesLoading"
      :rows="billing.rows"
      :total="billing.total"
      :t="t"
      @open-detail="openDetail($event.charge_id)"
      @page="billing.refreshCharges($event.first, $event.rows)"
    />
    <BillingPriceRulesPanel
      v-if="session.isAdmin"
      :endpoints="billing.endpoints"
      :first="billing.priceRuleFirst"
      :loading="billing.priceRulesLoading"
      :rules="billing.priceRules"
      :rows="billing.priceRuleRows"
      :total="billing.priceRuleTotal"
      :t="t"
      @create="openPriceRule"
      @edit="openPriceRule"
      @delete="deletePriceRule"
      @toggle="togglePriceRule"
      @page="billing.refreshPriceRules($event.first, $event.rows)"
    />
    <BillingPriceRuleDialog
      v-model:visible="priceRuleVisible"
      v-model:form="priceRuleForm"
      :busy="billing.loading"
      :endpoints="billing.endpoints"
      :t="t"
      @save="savePriceRule"
    />
    <BillingChargeDetailDialog
      v-model:visible="detailVisible"
      :detail="billing.detail"
      :is-admin="session.isAdmin"
      :loading="billing.detailLoading"
      :t="t"
    />
  </div>
</template>
