<script setup lang="ts">
import { computed, toRef } from 'vue'
import type { EndpointOAuthStatusResponse } from '@/generated/admin-api'
import EndpointOAuthBrowserLogin from '@/components/endpoints/EndpointOAuthBrowserLogin.vue'
import EndpointOAuthDeviceLogin from '@/components/endpoints/EndpointOAuthDeviceLogin.vue'
import { useEndpointOAuthActions } from '@/composables/useEndpointOAuthActions'
import {
  useTokenPlanTicker,
  useTokenPlanWindowEntries,
} from '@/composables/useTokenPlanWindowEntries'
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
const { usedPercent, progressColor, formatRemaining } =
  useTokenPlanWindowEntries(props.t, nowMs)

const hasToken = computed(() => status.value?.has_oauth_token ?? false)
const plan = computed(() => status.value?.plan ?? 'platform_api_key')
const expiresAt = computed(() => {
  const value = status.value?.expires_at
  return value ? new Date(value).toLocaleString() : ''
})
const needsLogin = computed(() => !hasToken.value)
const quotaEntry = computed(() => quota.value?.keys[0] ?? null)
const fiveHourWindow = computed(
  () => quotaEntry.value?.model_remains[0]?.interval ?? null,
)
const weeklyWindow = computed(
  () => quotaEntry.value?.model_remains[0]?.weekly ?? null,
)
const quotaRows = computed(() => {
  const rows: Array<{
    key: string
    labelKey: string
    window: NonNullable<typeof fiveHourWindow.value>
  }> = []
  if (fiveHourWindow.value) {
    rows.push({
      key: 'primary',
      labelKey: 'endpointOAuthQuotaPrimary',
      window: fiveHourWindow.value,
    })
  }
  if (weeklyWindow.value) {
    rows.push({
      key: 'secondary',
      labelKey: 'endpointOAuthQuotaSecondary',
      window: weeklyWindow.value,
    })
  }
  return rows
})
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
        <div
          v-for="row in quotaRows"
          :key="row.key"
          class="grid gap-1.5 sm:grid-cols-[7rem_minmax(0,1fr)_9rem] sm:items-center sm:gap-3"
        >
          <span class="text-dimmed">{{ t(row.labelKey) }}</span>
          <UProgress
            class="token-plan-progress h-1.5"
            :model-value="usedPercent(row.window)"
            :style="{
              '--token-plan-progress-color': progressColor(row.window),
            }"
          />
          <div class="flex items-center justify-between gap-2 text-xs">
            <span class="text-dimmed">{{ formatRemaining(row.window) }}</span>
            <span class="shrink-0 font-semibold"
              >{{ usedPercent(row.window).toFixed(1) }}%</span
            >
          </div>
        </div>
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

<style scoped>
.token-plan-progress :deep([data-slot='indicator']) {
  background-color: var(--token-plan-progress-color);
}
</style>
