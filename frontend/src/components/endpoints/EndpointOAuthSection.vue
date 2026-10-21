<script setup lang="ts">
import { computed, toRef } from 'vue'
import type { EndpointOAuthStatusResponse } from '@/generated/admin-api'
import EndpointOAuthBrowserLogin from '@/components/endpoints/EndpointOAuthBrowserLogin.vue'
import EndpointOAuthDeviceLogin from '@/components/endpoints/EndpointOAuthDeviceLogin.vue'
import { useEndpointOAuthActions } from '@/composables/useEndpointOAuthActions'
import { formatAppDateTime } from '@/format/datetime'
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

const hasToken = computed(() => status.value?.has_oauth_token ?? false)
const plan = computed(() => status.value?.plan ?? 'platform_api_key')
const expiresAt = computed(() => {
  const value = status.value?.expires_at
  return value ? formatAppDateTime(value) : ''
})
const needsLogin = computed(() => !hasToken.value)
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
