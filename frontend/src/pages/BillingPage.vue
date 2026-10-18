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
import { useBillingPageActions } from '@/composables/useBillingPageActions'
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
let detailOpenVersion = 0

const {
  deletePriceRule,
  priceRulePending,
  priceRuleSavePending,
  reprice,
  savePriceRule,
  togglePriceRule,
} = useBillingPageActions({
  billing,
  confirm: (message) => window.confirm(message),
  notifyApiError,
  notifySuccess,
  priceRuleForm,
  priceRuleVisible,
  t,
})

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
  const openVersion = ++detailOpenVersion
  detailVisible.value = true
  try {
    await billing.openDetail(chargeId)
  } catch (cause) {
    if (detailOpenVersion === openVersion) detailVisible.value = false
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
    <PageIntro>
      <template #actions>
        <div class="flex min-w-0 flex-1 flex-wrap justify-end gap-1.5">
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
            :loading="billing.repricePending"
            :disabled="billing.repricePending"
            @click="reprice"
            >{{ t('reprice') }}</UButton
          >
        </div>
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
    <BillingSummaryPanel :summary="billing.summary" :t="t" />
    <div class="grid min-w-0 gap-3">
      <BillingBreakdownPanel
        :rows="billing.summary?.by_client_key ?? []"
        :title="t('billingClientKeys')"
        :t="t"
      />
      <BillingBreakdownPanel
        :rows="billing.summary?.by_model ?? []"
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
      :first="billing.priceRuleFirst"
      :loading="billing.priceRulesLoading"
      :pending-actions="billing.pendingPriceRuleActions"
      :create-pending="billing.createPending"
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
      :busy="priceRulePending"
      :save-pending="priceRuleSavePending"
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
