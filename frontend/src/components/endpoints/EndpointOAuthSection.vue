<script setup lang="ts">
import { computed, toRef } from 'vue'
import type { EndpointOAuthStatusResponse } from '@/generated/admin-api'
import EndpointOAuthBrowserLogin from '@/components/endpoints/EndpointOAuthBrowserLogin.vue'
import EndpointOAuthDeviceLogin from '@/components/endpoints/EndpointOAuthDeviceLogin.vue'
import { useEndpointOAuthActions } from '@/composables/useEndpointOAuthActions'
import TokenPlanQuotaWindowRow from '@/components/endpoints/TokenPlanQuotaWindowRow.vue'
import {
  toCanonicalQuotaRows,
  type CanonicalQuotaRow,
} from '@/composables/useCanonicalQuotaWindows'
import { useTokenPlanTicker } from '@/composables/useTokenPlanWindowEntries'
import { subscriptionWindowLabel } from '@/composables/subscriptionQuotaWindows'
import type { RunEndpointOAuthAction } from '@/stores/endpoint-oauth-action-types'

const props = defineProps<{
  endpointId: string
  runAction?: RunEndpointOAuthAction
  t: TranslateFn
}>()

const emit = defineEmits<{
  status: [
    payload: Pick<EndpointOAuthStatusResponse, 'plan' | 'has_oauth_token'>,
  ]
}>()

const endpointId = toRef(props, 'endpointId')
const oauth = useEndpointOAuthActions({
  endpointId,
  onStatus: (payload) => emit('status', payload),
  runAction: props.runAction,
})
const {
  browserFlow,
  clearToken,
  clearing,
  deviceFlow,
  quota,
  quotaError,
  refreshToken,
  refreshing,
  status,
  statusError,
} = oauth
const {
  browser,
  busy: browserBusy,
  completeBrowser,
  message: browserMessage,
  redirectUrl,
  startBrowser,
} = browserFlow
const {
  device,
  message: deviceMessage,
  polling,
  startDevice,
  starting: deviceStarting,
} = deviceFlow
const tickerVisible = computed(() => quota.value !== null)
const nowMs = useTokenPlanTicker(tickerVisible)

const hasToken = computed(() => status.value?.has_oauth_token ?? false)
const plan = computed(() => status.value?.plan ?? 'platform_api_key')
const expiresAt = computed(() => {
  const value = status.value?.expires_at
  return value ? new Date(value).toLocaleString() : ''
})
const needsLogin = computed(() => !hasToken.value)
const quotaEntry = computed(() => quota.value?.keys[0] ?? null)
// Canonical time windows for the OAuth key: the same adapter rows the
// dialog renders, so subscription `model.windows` data shows the same
// correct values on both surfaces instead of the old hardcoded
// `model_remains[0].interval/weekly` pair. Balance/unlimited rows stay in
// their dedicated panels and never become progress bars here.
const quotaRows = computed<CanonicalQuotaRow[]>(() => {
  const entry = quotaEntry.value
  if (!entry) return []
  return toCanonicalQuotaRows(entry, nowMs.value).filter(
    (row) =>
      row.kind === 'five_hour' ||
      row.kind === 'rolling' ||
      row.kind === 'weekly' ||
      row.kind === 'monthly' ||
      row.kind === 'custom',
  )
})

// Label for one OAuth row: custom subscription windows use their actual
// duration, provider slots reuse the dialog copy. The component only
// receives the translated string, never a provider format.
function oauthRowLabel(row: CanonicalQuotaRow): string {
  if (row.kind === 'custom') {
    return subscriptionWindowLabel(
      {
        source_window: row.sourceId,
        window_seconds: row.durationSeconds,
        availability: row.availability,
        remaining_percent: row.remainingPercent,
        reset_at: row.resetAt,
      },
      props.t,
    )
  }
  if (row.sourceId.startsWith('command_code:')) {
    return props.t(
      row.kind === 'weekly' ? 'tokenPlanWeeklyUsd' : 'tokenPlanFiveHour',
    )
  }
  if (row.sourceId.startsWith('glm:')) {
    return props.t(
      row.kind === 'weekly' ? 'tokenPlanWeekly' : 'tokenPlanInterval',
    )
  }
  if (row.sourceId.startsWith('opencode_go:')) {
    if (row.kind === 'rolling') return props.t('tokenPlanRolling')
    if (row.kind === 'weekly') return props.t('tokenPlanWeekly')
    return props.t('tokenPlanMonthly')
  }
  if (row.sourceId.startsWith('minimax:')) {
    return props.t(
      row.kind === 'weekly' ? 'tokenPlanWeekly' : 'tokenPlanInterval',
    )
  }
  return props.t(
    row.kind === 'weekly' || row.kind === 'monthly'
      ? 'tokenPlanWeekly'
      : 'tokenPlanInterval',
  )
}

function oauthRowQuantitySuffix(row: CanonicalQuotaRow): string {
  return row.unit === 'currency' ? ' USD' : ''
}
const actionsEnabled = computed(() => props.runAction !== undefined)

function statusLabel(token: boolean, expired: boolean): string {
  if (!token) return props.t('endpointOAuthNotLoggedIn')
  return expired
    ? props.t('endpointOAuthExpired')
    : props.t('endpointOAuthLoggedIn')
}

function updateRedirectUrl(value: string): void {
  redirectUrl.value = value
}

async function copy(value: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value)
  } catch {
    // Clipboard access is optional; the value stays selectable on screen.
  }
}
</script>

<template>
  <div class="grid gap-3 rounded border border-default p-3">
    <div class="flex items-center justify-between gap-3">
      <div class="flex items-center gap-2">
        <span class="text-xs font-medium text-default">{{
          t('endpointOAuth')
        }}</span>
        <UBadge
          :label="statusLabel(hasToken, status?.expired ?? false)"
          :color="
            hasToken ? (status?.expired ? 'warning' : 'success') : 'neutral'
          "
          variant="subtle"
        />
        <UBadge
          v-if="hasToken"
          :label="
            plan === 'chatgpt_subscription'
              ? t('endpointPlanChatgptSubscription')
              : t('endpointPlanPlatformApiKey')
          "
          color="neutral"
          variant="subtle"
        />
      </div>
      <div class="flex items-center gap-2">
        <UButton
          v-if="hasToken"
          type="button"
          size="xs"
          color="neutral"
          variant="soft"
          icon="i-lucide-refresh-cw"
          :loading="refreshing"
          :disabled="!actionsEnabled"
          @click="refreshToken"
          >{{ t('endpointOAuthRefresh') }}</UButton
        >
        <UButton
          v-if="hasToken"
          type="button"
          size="xs"
          color="error"
          variant="soft"
          icon="i-lucide-trash-2"
          :loading="clearing"
          :disabled="!actionsEnabled"
          @click="clearToken"
          >{{ t('endpointOAuthClear') }}</UButton
        >
      </div>
    </div>
    <p v-if="statusError" class="break-words text-error">
      {{ statusError }}
    </p>
    <p v-if="hasToken && expiresAt" class="text-dimmed">
      {{ t('endpointOAuthExpiresAt') }}: {{ expiresAt }}
    </p>

    <div v-if="hasToken" class="grid gap-2 border-t border-default pt-3">
      <span class="text-xs font-medium text-default">{{
        t('endpointOAuthQuota')
      }}</span>
      <p class="text-xs text-dimmed">{{ t('endpointOAuthQuotaHint') }}</p>
      <template v-if="quotaRows.length">
        <TokenPlanQuotaWindowRow
          v-for="row in quotaRows"
          :key="row.sourceId"
          :row="row"
          :label="oauthRowLabel(row)"
          :t="t"
          :now-ms="nowMs"
          density="compact"
          :quantity-suffix="oauthRowQuantitySuffix(row)"
        />
      </template>
      <p v-else class="text-dimmed">
        {{ quotaError || t('endpointOAuthQuotaUnavailable') }}
      </p>
    </div>

    <div v-if="needsLogin" class="grid gap-3 border-t border-default pt-3">
      <EndpointOAuthDeviceLogin
        :actions-enabled="actionsEnabled"
        :device="device"
        :message="deviceMessage"
        :polling="polling"
        :starting="deviceStarting"
        :t="t"
        @copy="copy"
        @start="startDevice"
      />
      <EndpointOAuthBrowserLogin
        :actions-enabled="actionsEnabled"
        :browser="browser"
        :busy="browserBusy"
        :message="browserMessage"
        :redirect-url="redirectUrl"
        :t="t"
        @complete="completeBrowser"
        @copy="copy"
        @start="startBrowser"
        @update:redirect-url="updateRedirectUrl"
      />
    </div>
  </div>
</template>
