<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { User } from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import EndpointApiKeysEditor from '@/components/endpoints/EndpointApiKeysEditor.vue'
import EndpointProviderFields from '@/components/endpoints/EndpointProviderFields.vue'
import ProxySettingsFields from '@/components/shared/ProxySettingsFields.vue'
import ScheduleWindowsFields from '@/components/shared/ScheduleWindowsFields.vue'
import RequestLimitFields from '@/components/shared/RequestLimitFields.vue'

const props = defineProps<{
  busy: boolean
  header: string
  t: TranslateFn
  users: User[]
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<EndpointForm>('form', { required: true })

defineEmits<{
  save: []
}>()

// Whole-dialog two-level drill: main form plus settings sub-page.
// Gear swaps the entire modal content; back returns to the main form.
// Draft lives in `form` so edits persist across view switches and save
// through the existing outer save button.
const view = ref<'main' | 'settings'>('main')

function openSettings(): void {
  view.value = 'settings'
}

function backToMain(): void {
  view.value = 'main'
}

watch(visible, (open) => {
  if (!open) view.value = 'main'
})

// INLINE-proxy-ui-a1 BUG fix: guard legacy forms missing Phase C fields so
// the dialog always renders instead of throwing on undefined access.
const hasProxy = computed(() => {
  const typed = (form.value?.proxy_url ?? '').trim() !== ''
  const saved = form.value?.has_saved_proxy_url ?? false
  return typed || saved
})

// Issue #368 Phase C: proxy validity for the outer save. Empty means
// direct/keep (valid); `scheme://` with an empty address is an unfinished
// inline edit and must block the outer save.
const isProxyValid = computed(() => {
  const trimmed = (form.value?.proxy_url ?? '').trim()
  if (!trimmed) return true
  const match = trimmed.match(/^(http|https|socks5h|socks5):\/\/(.*)$/i)
  if (match) return (match[2] ?? '').trim() !== ''
  return true
})

const HHMM_RE = /^([01]\d|2[0-3]):[0-5]\d$/

function endpointRowError(window: { start: string; end: string }): string {
  const start = (window?.start ?? '').trim()
  const end = (window?.end ?? '').trim()
  if (!start || !end) return props.t('scheduleRequired')
  if (!HHMM_RE.test(start) || !HHMM_RE.test(end))
    return props.t('scheduleInvalid')
  if (start === end) return props.t('scheduleEqual')
  return ''
}

const isScheduleValid = computed(() =>
  (form.value?.active_windows ?? []).every(
    (window) => endpointRowError(window) === '',
  ),
)

const canSaveOuter = computed(() => isProxyValid.value && isScheduleValid.value)

// Issue #392 Phase L: endpoint default schedule mirrors the target dialog.
// Non-empty means restricted; empty means all-day.
function endpointWindows(): Array<{ start: string; end: string }> {
  return Array.isArray(form.value?.active_windows)
    ? (form.value?.active_windows ?? [])
    : []
}

function sortedEndpointWindows(): Array<{ start: string; end: string }> {
  return [...endpointWindows()]
    .map((window) => ({
      start: (window?.start ?? '').trim(),
      end: (window?.end ?? '').trim(),
    }))
    .filter((window) => window.start !== '' && window.end !== '')
    .sort((a, b) =>
      a.start === b.start
        ? a.end.localeCompare(b.end)
        : a.start.localeCompare(b.start),
    )
}

function hasEndpointSchedule(): boolean {
  return sortedEndpointWindows().length > 0
}

// Gear highlight when anything is non-default.
function hasEndpointSettings(): boolean {
  return hasProxy.value || hasEndpointSchedule()
}
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{ content: 'sm:max-w-4xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
        <template v-if="view === 'main'">
          <EndpointProviderFields v-model:form="form" :t="t" />
          <USelect
            v-if="form.scope === 'user'"
            :model-value="form.owner_user_id ?? undefined"
            class="w-full"
            :items="users"
            label-key="login_name"
            value-key="user_id"
            :placeholder="t('ownerUser')"
            @update:model-value="form.owner_user_id = $event ?? null"
          />
          <EndpointApiKeysEditor v-model:form="form" :t="t" />
          <div class="grid gap-2 border-t border-default pt-3">
            <div class="flex items-center justify-between gap-3">
              <div class="flex items-center gap-1">
                <span class="text-xs font-medium text-default">
                  {{ t('endpointSettings') }}
                </span>
                <UTooltip :text="t('endpointSettingsHint')">
                  <UButton
                    type="button"
                    size="xs"
                    color="neutral"
                    variant="ghost"
                    icon="i-lucide-info"
                    :aria-label="t('endpointSettingsHint')"
                  />
                </UTooltip>
              </div>
              <UButton
                type="button"
                size="sm"
                :color="hasEndpointSettings() ? 'primary' : 'neutral'"
                variant="ghost"
                icon="i-lucide-settings-2"
                :aria-label="t('endpointSettings')"
                :aria-pressed="hasEndpointSettings()"
                :title="t('endpointSettingsHint')"
                @click="openSettings"
              />
            </div>
          </div>
          <div
            v-if="form.provider === 'minimax'"
            class="flex items-center justify-between gap-3 border-t border-default pt-3"
          >
            <div class="flex items-center gap-1">
              <label
                for="endpoint-minimax-mcp"
                class="text-xs font-medium text-default"
              >
                {{ t('minimaxMcp') }}
              </label>
              <UTooltip :text="t('minimaxMcpHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('minimaxMcpHint')"
                />
              </UTooltip>
            </div>
            <USwitch id="endpoint-minimax-mcp" v-model="form.mcp_enabled" />
          </div>
          <RequestLimitFields
            v-model:form="form"
            daily-label="dailyRequestLimit"
            monthly-label="monthlyRequestLimit"
            :t="t"
          />
        </template>
        <template v-else>
          <div class="flex items-center gap-1">
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-arrow-left"
              :aria-label="t('cancel')"
              @click="backToMain"
            />
            <span class="text-sm font-medium text-default">{{
              t('endpointSettings')
            }}</span>
          </div>
          <div class="grid gap-2 rounded border border-default bg-muted p-3">
            <div class="flex items-center gap-1">
              <span class="text-xs font-medium text-default">{{
                t('proxyUrl')
              }}</span>
              <UTooltip :text="t('proxyUrlHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('proxyUrlHint')"
                />
              </UTooltip>
            </div>
            <ProxySettingsFields
              v-model:proxy-url="form.proxy_url"
              v-model:has-saved="form.has_saved_proxy_url"
              :hint="t('proxyUrlHint')"
              :t="t"
            />
          </div>
          <div class="grid gap-2 rounded border border-default bg-muted p-3">
            <div class="flex items-center gap-1">
              <span class="text-xs font-medium text-default">{{
                t('scheduleWindows')
              }}</span>
              <UTooltip :text="t('scheduleWindowsHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('scheduleWindowsHint')"
                />
              </UTooltip>
            </div>
            <ScheduleWindowsFields
              v-model:windows="form.active_windows"
              v-model:touched="form.active_windows_touched"
              :hint="t('scheduleWindowsHint')"
              :t="t"
            />
          </div>
        </template>
        <div class="flex justify-end gap-2 pt-1">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            @click="
              () => {
                visible = false
              }
            "
            >{{ t('cancel') }}</UButton
          >
          <UButton
            type="submit"
            size="sm"
            :loading="busy"
            :disabled="!canSaveOuter"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('saveEndpoint')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
