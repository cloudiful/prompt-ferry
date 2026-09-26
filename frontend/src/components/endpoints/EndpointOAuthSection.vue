<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type {
  EndpointOAuthStatusResponse,
  OAuthBrowserStartResponse,
  OAuthDeviceStartResponse,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import {
  oauthBrowserComplete,
  oauthBrowserStart,
  oauthClear,
  oauthDevicePoll,
  oauthDeviceStart,
  oauthRefresh,
  oauthStatus,
  tokenPlanUsage,
} from '@/generated/admin-api'
import { expectData, formatApiError, withData } from '@/api'
import {
  useTokenPlanTicker,
  useTokenPlanWindowEntries,
} from '@/composables/useTokenPlanWindowEntries'

const props = defineProps<{
  endpointId: string
  t: TranslateFn
}>()

const emit = defineEmits<{
  status: [
    payload: {
      plan: EndpointOAuthStatusResponse['plan']
      has_oauth_token: boolean
    },
  ]
}>()

const status = ref<EndpointOAuthStatusResponse | null>(null)
const quota = ref<TokenPlanUsageResponse | null>(null)
const quotaError = ref('')
const statusError = ref('')
const refreshing = ref(false)
const clearing = ref(false)
const device = ref<OAuthDeviceStartResponse | null>(null)
const deviceBusy = ref(false)
const deviceMessage = ref('')
const browser = ref<OAuthBrowserStartResponse | null>(null)
const browserBusy = ref(false)
const browserMessage = ref('')
const redirectUrl = ref('')
let pollTimer: ReturnType<typeof setInterval> | null = null

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
// Issue #599 R2c: the ChatGPT quota adapter returns one model-remains entry
// whose interval/weekly pair are the 5h and weekly subscription windows.
const fiveHourWindow = computed(
  () => quotaEntry.value?.model_remains[0]?.interval ?? null,
)
const weeklyWindow = computed(
  () => quotaEntry.value?.model_remains[0]?.weekly ?? null,
)
const hasQuotaWindows = computed(() => quotaRows.value.length > 0)
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

function statusLabel(token: boolean, expired: boolean): string {
  if (!token) return props.t('endpointOAuthNotLoggedIn')
  return expired
    ? props.t('endpointOAuthExpired')
    : props.t('endpointOAuthLoggedIn')
}

async function loadStatus(): Promise<void> {
  statusError.value = ''
  try {
    const next = expectData(
      await oauthStatus<true>(
        withData({ path: { endpoint_id: props.endpointId } }),
      ),
    )
    status.value = next
    emit('status', {
      plan: next.plan,
      has_oauth_token: next.has_oauth_token,
    })
    if (next.has_oauth_token) await loadQuota()
    else {
      quota.value = null
      quotaError.value = ''
    }
  } catch (error) {
    statusError.value = formatApiError(error)
  }
}

async function loadQuota(): Promise<void> {
  quotaError.value = ''
  try {
    quota.value = expectData(
      await tokenPlanUsage<true>(
        withData({ path: { endpoint_id: props.endpointId } }),
      ),
    )
  } catch (error) {
    quota.value = null
    quotaError.value = formatApiError(error)
  }
}

function stopPolling(): void {
  if (pollTimer !== null) {
    clearInterval(pollTimer)
    pollTimer = null
  }
}

async function startDevice(): Promise<void> {
  deviceMessage.value = ''
  deviceBusy.value = true
  try {
    device.value = expectData(
      await oauthDeviceStart<true>(
        withData({ path: { endpoint_id: props.endpointId } }),
      ),
    )
    scheduleDevicePoll()
  } catch (error) {
    deviceMessage.value = formatApiError(error)
  } finally {
    deviceBusy.value = false
  }
}

function scheduleDevicePoll(): void {
  stopPolling()
  const seconds = Math.max(1, device.value?.interval_seconds ?? 5)
  pollTimer = setInterval(() => {
    void pollDevice()
  }, seconds * 1000)
}

async function pollDevice(): Promise<void> {
  const flow = device.value
  if (!flow) return
  try {
    const result = expectData(
      await oauthDevicePoll<true>(
        withData({
          path: { endpoint_id: props.endpointId },
          body: { flow_id: flow.flow_id },
        }),
      ),
    )
    if (result.status === 'complete') {
      stopPolling()
      device.value = null
      await loadStatus()
    }
  } catch (error) {
    // A terminal flow error (expired/denied/upstream) ends the wait; the
    // operator can start a fresh login.
    stopPolling()
    device.value = null
    deviceMessage.value = formatApiError(error)
  }
}

async function startBrowser(): Promise<void> {
  browserMessage.value = ''
  browserBusy.value = true
  try {
    browser.value = expectData(
      await oauthBrowserStart<true>(
        withData({ path: { endpoint_id: props.endpointId } }),
      ),
    )
  } catch (error) {
    browserMessage.value = formatApiError(error)
  } finally {
    browserBusy.value = false
  }
}

async function completeBrowser(): Promise<void> {
  const flow = browser.value
  if (!flow) return
  browserBusy.value = true
  browserMessage.value = ''
  try {
    const result = expectData(
      await oauthBrowserComplete<true>(
        withData({
          path: { endpoint_id: props.endpointId },
          body: {
            flow_id: flow.flow_id,
            redirect_url: redirectUrl.value.trim(),
          },
        }),
      ),
    )
    if (result.status === 'complete') {
      browser.value = null
      redirectUrl.value = ''
      await loadStatus()
    }
  } catch (error) {
    browserMessage.value = formatApiError(error)
  } finally {
    browserBusy.value = false
  }
}

async function refreshToken(): Promise<void> {
  refreshing.value = true
  statusError.value = ''
  try {
    status.value = expectData(
      await oauthRefresh<true>(
        withData({ path: { endpoint_id: props.endpointId } }),
      ),
    )
    await loadQuota()
  } catch (error) {
    statusError.value = formatApiError(error)
  } finally {
    refreshing.value = false
  }
}

async function clearToken(): Promise<void> {
  clearing.value = true
  statusError.value = ''
  try {
    await oauthClear<true>(
      withData({ path: { endpoint_id: props.endpointId } }),
    )
    await loadStatus()
  } catch (error) {
    statusError.value = formatApiError(error)
  } finally {
    clearing.value = false
  }
}

async function copy(value: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value)
  } catch {
    // Clipboard access is optional; the value stays selectable on screen.
  }
}

onMounted(() => {
  if (props.endpointId) void loadStatus()
})

onBeforeUnmount(stopPolling)

watch(
  () => props.endpointId,
  () => {
    stopPolling()
    status.value = null
    quota.value = null
    quotaError.value = ''
    statusError.value = ''
    device.value = null
    browser.value = null
    redirectUrl.value = ''
    if (props.endpointId) void loadStatus()
  },
)
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
          @click="clearToken"
          >{{ t('endpointOAuthClear') }}</UButton
        >
      </div>
    </div>
    <p v-if="statusError" class="break-words text-error">{{ statusError }}</p>
    <p v-if="hasToken && expiresAt" class="text-dimmed">
      {{ t('endpointOAuthExpiresAt') }}: {{ expiresAt }}
    </p>

    <div v-if="hasToken" class="grid gap-2 border-t border-default pt-3">
      <span class="text-xs font-medium text-default">{{
        t('endpointOAuthQuota')
      }}</span>
      <p class="text-xs text-dimmed">{{ t('endpointOAuthQuotaHint') }}</p>
      <template v-if="hasQuotaWindows">
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
      <div class="grid gap-2">
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs font-medium text-default">{{
            t('endpointOAuthDeviceLogin')
          }}</span>
          <UButton
            type="button"
            size="xs"
            color="primary"
            variant="soft"
            :loading="deviceBusy"
            @click="startDevice"
            >{{ t('endpointOAuthDeviceStart') }}</UButton
          >
        </div>
        <p class="text-xs text-dimmed">{{ t('endpointOAuthDeviceHint') }}</p>
        <template v-if="device">
          <div class="flex items-center gap-2">
            <code class="rounded bg-muted px-2 py-1 font-mono text-sm">{{
              device.user_code
            }}</code>
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-copy"
              :aria-label="t('endpointOAuthCopy')"
              @click="copy(device.user_code)"
            />
          </div>
          <p class="text-xs text-dimmed">
            {{ t('endpointOAuthDeviceWaiting') }}
            <a
              class="underline"
              :href="device.verification_uri"
              target="_blank"
              rel="noreferrer"
              >{{ device.verification_uri }}</a
            >
          </p>
        </template>
        <p v-if="deviceMessage" class="break-words text-error">
          {{ deviceMessage }}
        </p>
      </div>

      <div class="grid gap-2 border-t border-default pt-3">
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs font-medium text-default">{{
            t('endpointOAuthBrowserLogin')
          }}</span>
          <UButton
            type="button"
            size="xs"
            color="primary"
            variant="soft"
            :loading="browserBusy"
            @click="startBrowser"
            >{{ t('endpointOAuthBrowserStart') }}</UButton
          >
        </div>
        <p class="text-xs text-dimmed">{{ t('endpointOAuthBrowserHint') }}</p>
        <template v-if="browser">
          <div class="grid gap-1">
            <label class="text-xs text-muted" for="endpoint-oauth-authorize">
              {{ t('endpointOAuthAuthorizeUrl') }}
            </label>
            <div class="flex items-center gap-2">
              <UInput
                id="endpoint-oauth-authorize"
                class="w-full"
                :model-value="browser.authorize_url"
                readonly
              />
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-copy"
                :aria-label="t('endpointOAuthCopy')"
                @click="copy(browser.authorize_url)"
              />
            </div>
          </div>
          <div class="grid gap-1">
            <label class="text-xs text-muted" for="endpoint-oauth-redirect">
              {{ t('endpointOAuthRedirectUrl') }}
            </label>
            <UInput
              id="endpoint-oauth-redirect"
              v-model="redirectUrl"
              class="w-full"
              :placeholder="t('endpointOAuthRedirectPlaceholder')"
            />
          </div>
          <div class="flex justify-end">
            <UButton
              type="button"
              size="xs"
              color="primary"
              :loading="browserBusy"
              :disabled="redirectUrl.trim() === ''"
              @click="completeBrowser"
              >{{ t('endpointOAuthComplete') }}</UButton
            >
          </div>
        </template>
        <p v-if="browserMessage" class="break-words text-error">
          {{ browserMessage }}
        </p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.token-plan-progress :deep([data-slot='indicator']) {
  background-color: var(--token-plan-progress-color);
}
</style>
