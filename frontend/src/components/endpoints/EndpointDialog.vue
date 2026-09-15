<script setup lang="ts">
import { computed, ref } from 'vue'
import type { User } from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import EndpointApiKeysEditor from '@/components/endpoints/EndpointApiKeysEditor.vue'
import EndpointProviderFields from '@/components/endpoints/EndpointProviderFields.vue'
import ProxySettingsDialog from '@/components/shared/ProxySettingsDialog.vue'
import ScheduleWindowsDialog from '@/components/shared/ScheduleWindowsDialog.vue'
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

const proxyModalOpen = ref(false)

// INLINE-proxy-ui-a1 BUG fix: guard legacy forms missing Phase C fields so
// the dialog always renders instead of throwing on undefined access.
const hasProxy = computed(() => {
  const typed = (form.value?.proxy_url ?? '').trim() !== ''
  const saved = form.value?.has_saved_proxy_url ?? false
  return typed || saved
})

// Issue #368 Phase C: clear the saved proxy so the next save sends `""`
// (clear to direct). Leaving the field blank while `has_saved_proxy_url`
// is true omits the key and keeps the stored value (see
// `endpointFormToRequest`).
function clearProxyUrl(): void {
  if (!form.value) return
  form.value.proxy_url = ''
  form.value.has_saved_proxy_url = false
}

function onProxySave(value: string): void {
  if (!form.value) return
  const trimmed = (value ?? '').trim()
  form.value.proxy_url = trimmed
  // Saving direct (`""`) must clear the saved flag so the next save sends
  // `""` (clear) instead of omitting the key (keep).
  if (trimmed === '') form.value.has_saved_proxy_url = false
}

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

function formatEndpointWindow(window: { start: string; end: string }): string {
  return `${window.start}–${window.end}`
}

function endpointScheduleSummary(): string {
  const windows = sortedEndpointWindows()
  if (windows.length === 0) return props.t('scheduleAllDay')
  const first = windows[0]
  if (!first) return props.t('scheduleAllDay')
  if (windows.length === 1) return formatEndpointWindow(first)
  return `${formatEndpointWindow(first)} ${props.t('scheduleMoreWindows', { count: windows.length })}`
}

function endpointScheduleTooltip(): string {
  const windows = sortedEndpointWindows()
  if (windows.length === 0) return props.t('scheduleAllDay')
  return windows.map(formatEndpointWindow).join(', ')
}

// Issue #392 Phase L: concise proxy summary (plain text, no pill).
function endpointProxySummary(): string {
  return hasProxy.value ? props.t('proxySet') : props.t('proxyDirectShort')
}

// Issue #392 Phase L: gear highlight when anything is non-default.
function hasEndpointSettings(): boolean {
  return hasProxy.value || hasEndpointSchedule()
}

function onEndpointScheduleSave(
  value: Array<{ start: string; end: string }>,
): void {
  if (!form.value) return
  form.value.active_windows = value.map((window) => ({
    start: (window?.start ?? '').trim(),
    end: (window?.end ?? '').trim(),
  }))
  form.value.active_windows_touched = true
}

const scheduleModalOpen = ref(false)
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{ content: 'sm:max-w-4xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
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
        <div
          class="flex items-center justify-between gap-3 border-t border-default pt-3"
        >
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
          <UPopover
            :content="{
              side: 'bottom',
              align: 'end',
              sideOffset: 6,
              collisionPadding: 8,
            }"
          >
            <UButton
              type="button"
              size="sm"
              :color="hasEndpointSettings() ? 'primary' : 'neutral'"
              variant="ghost"
              icon="i-lucide-settings-2"
              :aria-label="t('endpointSettings')"
              :aria-pressed="hasEndpointSettings()"
              :title="t('endpointSettingsHint')"
            />
            <template #content>
              <div
                class="grid w-[min(20rem,calc(100vw-2rem))] gap-2 p-3 text-xs"
              >
                <div class="flex items-center justify-between gap-2">
                  <div class="flex min-w-0 items-center gap-1">
                    <span class="font-medium text-default">{{
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
                  <div class="flex shrink-0 items-center gap-1">
                    <span class="text-muted">{{ endpointProxySummary() }}</span>
                    <UButton
                      type="button"
                      size="xs"
                      color="neutral"
                      variant="ghost"
                      icon="i-lucide-pencil"
                      :aria-label="t('proxySettings')"
                      @click="proxyModalOpen = true"
                    />
                  </div>
                </div>
                <div class="flex items-center justify-between gap-2">
                  <div class="flex min-w-0 items-center gap-1">
                    <span class="font-medium text-default">{{
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
                  <div class="flex shrink-0 items-center gap-1">
                    <UTooltip :text="endpointScheduleTooltip()">
                      <span class="text-muted">{{
                        endpointScheduleSummary()
                      }}</span>
                    </UTooltip>
                    <UButton
                      type="button"
                      size="xs"
                      color="neutral"
                      variant="ghost"
                      icon="i-lucide-pencil"
                      :aria-label="t('scheduleSettings')"
                      @click="scheduleModalOpen = true"
                    />
                  </div>
                </div>
              </div>
            </template>
          </UPopover>
        </div>
        <ProxySettingsDialog
          v-model:visible="proxyModalOpen"
          :initial-value="form?.proxy_url ?? ''"
          :has-saved="form?.has_saved_proxy_url ?? false"
          :hint="t('proxyUrlHint')"
          :t="t"
          @save="onProxySave"
          @clear="clearProxyUrl"
        />
        <ScheduleWindowsDialog
          v-model:visible="scheduleModalOpen"
          :initial-value="form?.active_windows ?? []"
          :hint="t('scheduleWindowsHint')"
          :t="t"
          @save="onEndpointScheduleSave"
        />
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
          <UButton type="submit" size="sm" :loading="busy"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('saveEndpoint')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
