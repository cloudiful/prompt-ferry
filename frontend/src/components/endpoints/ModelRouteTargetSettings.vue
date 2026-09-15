<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import ProxySettingsForm from '@/components/shared/ProxySettingsForm.vue'
import ScheduleWindowsForm from '@/components/shared/ScheduleWindowsForm.vue'
import type { ModelRouteTargetForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
}>()

const target = defineModel<ModelRouteTargetForm>('target', { required: true })

// Two-level drill-in inside the same settings panel: level one lists proxy
// row plus schedule row plus inline normalize toggle, level two renders the
// existing proxy/schedule form. No second overlay layer opens at any point.
const popoverOpen = ref(false)
const view = ref<'main' | 'proxy' | 'schedule'>('main')

function backToMain(): void {
  view.value = 'main'
}

watch(popoverOpen, (open) => {
  if (!open) view.value = 'main'
})

function hasTargetProxy(): boolean {
  return (
    (target.value?.proxy_url_override ?? '').trim() !== '' ||
    (target.value?.has_saved_proxy_url_override ?? false)
  )
}

// Issue #392 Phase L: concise proxy summary (plain text, no pill).
// Set when an override is typed or saved; otherwise inherit.
const proxySummaryText = computed(() =>
  hasTargetProxy() ? props.t('proxySet') : props.t('proxyInherit'),
)

// Issue #378 Phase J: per-target schedule windows. Non-empty means
// restricted; empty means all-day.
function targetWindows(): Array<{ start: string; end: string }> {
  return Array.isArray(target.value?.active_windows)
    ? (target.value?.active_windows ?? [])
    : []
}

function sortedTargetWindows(): Array<{ start: string; end: string }> {
  return [...targetWindows()]
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

function hasTargetSchedule(): boolean {
  return sortedTargetWindows().length > 0
}

function formatWindow(window: { start: string; end: string }): string {
  return `${window.start}–${window.end}`
}

function scheduleSummaryText(): string {
  const windows = sortedTargetWindows()
  if (windows.length === 0) return props.t('scheduleAllDay')
  const first = windows[0]
  if (!first) return props.t('scheduleAllDay')
  if (windows.length === 1) return formatWindow(first)
  return `${formatWindow(first)} ${props.t('scheduleMoreWindows', { count: windows.length })}`
}

function scheduleTooltipText(): string {
  const windows = sortedTargetWindows()
  if (windows.length === 0) return props.t('scheduleAllDay')
  return windows.map(formatWindow).join(', ')
}

// Issue #392 Phase L: gear highlight when anything is non-default
// (proxy set, schedule restricted, or normalize on).
function hasTargetSettings(): boolean {
  return (
    hasTargetProxy() ||
    hasTargetSchedule() ||
    (target.value?.dev_system_normalize ?? false)
  )
}

// Issue #368 Phase C: clear the saved override so the next save sends `""`
// (clear to inherit). Leaving the field blank while
// `has_saved_proxy_url_override` is true omits the key and keeps the
// stored value (see `modelRouteFormToRequest`).
function clearTargetProxyOverride(): void {
  if (!target.value) return
  target.value.proxy_url_override = ''
  target.value.has_saved_proxy_url_override = false
  backToMain()
}

function onTargetProxySave(value: string): void {
  if (!target.value) return
  const trimmed = (value ?? '').trim()
  target.value.proxy_url_override = trimmed
  if (trimmed === '') target.value.has_saved_proxy_url_override = false
  backToMain()
}

function onTargetScheduleSave(
  value: Array<{ start: string; end: string }>,
): void {
  if (!target.value) return
  target.value.active_windows = value.map((window) => ({
    start: (window?.start ?? '').trim(),
    end: (window?.end ?? '').trim(),
  }))
  target.value.active_windows_touched = true
  backToMain()
}
</script>

<template>
  <UPopover
    v-model:open="popoverOpen"
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
      :color="hasTargetSettings() ? 'primary' : 'neutral'"
      variant="ghost"
      icon="i-lucide-settings-2"
      :aria-label="t('targetSettings')"
      :aria-pressed="hasTargetSettings()"
      :title="t('targetSettingsHint')"
    />
    <template #content>
      <div
        v-if="view === 'main'"
        class="grid w-[min(20rem,calc(100vw-2rem))] gap-2 p-3 text-xs"
      >
        <div
          role="button"
          tabindex="0"
          class="flex cursor-pointer items-center justify-between gap-2 rounded px-1 py-1 hover:bg-elevated"
          @click="view = 'proxy'"
          @keydown.enter="view = 'proxy'"
          @keydown.space.prevent="view = 'proxy'"
        >
          <div class="flex min-w-0 items-center gap-1">
            <span class="font-medium text-default">{{
              t('proxyUrlOverride')
            }}</span>
            <UTooltip :text="t('proxyUrlOverrideHint')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('proxyUrlOverrideHint')"
                @click.stop
              />
            </UTooltip>
          </div>
          <div class="flex shrink-0 items-center gap-1">
            <span class="text-muted">{{ proxySummaryText }}</span>
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-pencil"
              :aria-label="t('proxySettings')"
              @click.stop="view = 'proxy'"
            />
          </div>
        </div>
        <div
          role="button"
          tabindex="0"
          class="flex cursor-pointer items-center justify-between gap-2 rounded px-1 py-1 hover:bg-elevated"
          @click="view = 'schedule'"
          @keydown.enter="view = 'schedule'"
          @keydown.space.prevent="view = 'schedule'"
        >
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
                @click.stop
              />
            </UTooltip>
          </div>
          <div class="flex shrink-0 items-center gap-1">
            <UTooltip :text="scheduleTooltipText()">
              <span class="text-muted">{{ scheduleSummaryText() }}</span>
            </UTooltip>
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-pencil"
              :aria-label="t('scheduleSettings')"
              @click.stop="view = 'schedule'"
            />
          </div>
        </div>
        <div class="flex items-center justify-between gap-2 px-1 py-1">
          <div class="flex min-w-0 items-center gap-1">
            <span class="font-medium text-default">{{
              t('normalizeLabel')
            }}</span>
            <UTooltip :text="t('normalizeHint')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('normalizeHint')"
                @click.stop
              />
            </UTooltip>
          </div>
          <USwitch
            v-model="target.dev_system_normalize"
            :aria-label="t('normalizeLabel')"
          />
        </div>
      </div>
      <div
        v-else-if="view === 'proxy'"
        class="grid w-[min(20rem,calc(100vw-2rem))] gap-2 p-3 text-xs"
      >
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
          <span class="font-medium text-default">{{ t('proxySettings') }}</span>
        </div>
        <ProxySettingsForm
          :key="`target-proxy-${target?.proxy_url_override ?? ''}-${target?.has_saved_proxy_url_override ?? false}`"
          :initial-value="target?.proxy_url_override ?? ''"
          :has-saved="target?.has_saved_proxy_url_override ?? false"
          :hint="t('proxyUrlOverrideHint')"
          :t="t"
          @save="onTargetProxySave"
          @clear="clearTargetProxyOverride"
          @cancel="backToMain"
        />
      </div>
      <div
        v-else
        class="grid w-[min(20rem,calc(100vw-2rem))] gap-2 p-3 text-xs"
      >
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
          <span class="font-medium text-default">{{
            t('scheduleSettings')
          }}</span>
        </div>
        <ScheduleWindowsForm
          :key="`target-schedule-${JSON.stringify(target?.active_windows ?? [])}`"
          :initial-value="target?.active_windows ?? []"
          :hint="t('scheduleWindowsHint')"
          :t="t"
          @save="onTargetScheduleSave"
          @cancel="backToMain"
        />
      </div>
    </template>
  </UPopover>
</template>
