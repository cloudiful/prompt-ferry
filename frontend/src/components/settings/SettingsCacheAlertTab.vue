<script setup lang="ts">
import { computed } from 'vue'
import type { CacheAlertForm } from '@/admin-mappers'
import SettingsCard from './SettingsCard.vue'

defineProps<{
  busy: boolean
  error?: string | null
  t: TranslateFn
}>()

const cacheAlert = defineModel<CacheAlertForm | null>('cacheAlert', {
  required: true,
})

defineEmits<{
  saveCacheAlert: []
  retryCacheAlert: []
}>()

const secretInput = computed({
  get: () => cacheAlert.value?.secret.value ?? '',
  set: (value: string) => {
    const form = cacheAlert.value
    if (!form) return
    form.secret.value = value
    form.secret.mode = value.trim() ? 'replace' : 'keep'
  },
})

const secretReplacing = computed(
  () => cacheAlert.value?.secret.mode === 'replace',
)

const secretConfigured = computed(
  () => cacheAlert.value?.has_dingtalk_secret ?? false,
)
</script>

<template>
  <section class="grid gap-3">
    <SettingsCard class="lg:col-span-full">
      <template #header>
        <h3
          class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
        >
          <UIcon name="i-lucide-bell-ring" class="h-3.5 w-3.5 text-muted" />
          {{ t('cacheAlertTitle') }}
        </h3>
        <div class="flex flex-wrap items-center gap-2">
          <USwitch
            v-if="cacheAlert"
            id="settings-cache-alert-enabled"
            v-model="cacheAlert.enabled"
            :aria-label="t('cacheAlertEnabled')"
          />
          <UButton
            size="sm"
            icon="i-lucide-save"
            :loading="busy"
            :disabled="!cacheAlert"
            @click="$emit('saveCacheAlert')"
          >
            {{ t('save') }}
          </UButton>
        </div>
      </template>

      <div
        v-if="!cacheAlert && !busy"
        class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2"
      >
        <p
          class="m-0 flex items-center gap-1.5 text-xs leading-relaxed text-muted"
        >
          <UIcon
            name="i-lucide-triangle-alert"
            class="h-3.5 w-3.5 shrink-0 text-warning"
          />
          {{ t('cacheAlertUnavailable') }}
        </p>
        <p v-if="error" class="m-0 mt-1 text-[11px] text-muted">
          {{ error }}
        </p>
        <div class="mt-2">
          <UButton
            size="xs"
            color="neutral"
            variant="soft"
            icon="i-lucide-refresh-cw"
            @click="$emit('retryCacheAlert')"
          >
            {{ t('refresh') }}
          </UButton>
        </div>
      </div>

      <div v-else-if="cacheAlert" class="grid gap-3 md:grid-cols-2">
        <label class="grid gap-1">
          <span
            class="inline-flex items-center gap-1 text-xs font-medium text-muted"
          >
            {{ t('cacheAlertWindowMinutes') }}
            <UTooltip :text="t('cacheAlertWindowMinutesHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('cacheAlertWindowMinutesHelp')"
              />
            </UTooltip>
          </span>
          <UInputNumber
            v-model="cacheAlert.window_minutes"
            size="sm"
            :min="5"
            :max="1440"
            :use-grouping="false"
            :placeholder="t('cacheAlertWindowMinutesPlaceholder')"
          />
        </label>

        <label class="grid gap-1">
          <span
            class="inline-flex items-center gap-1 text-xs font-medium text-muted"
          >
            {{ t('cacheAlertMinTurns') }}
            <UTooltip :text="t('cacheAlertMinTurnsHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('cacheAlertMinTurnsHelp')"
              />
            </UTooltip>
          </span>
          <UInputNumber
            v-model="cacheAlert.min_turns"
            size="sm"
            :min="2"
            :max="100"
            :use-grouping="false"
            :placeholder="t('cacheAlertMinTurnsPlaceholder')"
          />
        </label>

        <label class="grid gap-1">
          <span
            class="inline-flex items-center gap-1 text-xs font-medium text-muted"
          >
            {{ t('cacheAlertThreshold') }}
            <UTooltip :text="t('cacheAlertThresholdHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('cacheAlertThresholdHelp')"
              />
            </UTooltip>
          </span>
          <UInputNumber
            v-model="cacheAlert.threshold"
            size="sm"
            :min="0"
            :max="1"
            :step="0.05"
            :use-grouping="false"
            :placeholder="t('cacheAlertThresholdPlaceholder')"
          />
        </label>

        <label class="grid gap-1">
          <span
            class="inline-flex items-center gap-1 text-xs font-medium text-muted"
          >
            {{ t('cacheAlertCooldownMinutes') }}
            <UTooltip :text="t('cacheAlertCooldownMinutesHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('cacheAlertCooldownMinutesHelp')"
              />
            </UTooltip>
          </span>
          <UInputNumber
            v-model="cacheAlert.cooldown_minutes"
            size="sm"
            :min="5"
            :max="1440"
            :use-grouping="false"
            :placeholder="t('cacheAlertCooldownMinutesPlaceholder')"
          />
        </label>

        <label class="grid gap-1 md:col-span-2">
          <span
            class="inline-flex items-center gap-1 text-xs font-medium text-muted"
          >
            {{ t('cacheAlertWebhookUrl') }}
            <UTooltip :text="t('cacheAlertWebhookUrlHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('cacheAlertWebhookUrlHelp')"
              />
            </UTooltip>
          </span>
          <UInput
            v-model="cacheAlert.dingtalk_webhook_url"
            size="sm"
            :placeholder="t('cacheAlertWebhookUrlPlaceholder')"
          />
        </label>

        <div class="grid gap-1 md:col-span-2">
          <span
            class="inline-flex items-center gap-1.5 text-xs font-medium text-muted"
          >
            {{ t('cacheAlertSecret') }}
            <UBadge
              :color="secretConfigured ? 'success' : 'neutral'"
              variant="subtle"
              size="xs"
              :label="
                secretConfigured
                  ? t('cacheAlertSecretConfigured')
                  : t('cacheAlertSecretNotConfigured')
              "
            />
            <UBadge
              :color="secretReplacing ? 'warning' : 'neutral'"
              variant="subtle"
              size="xs"
              :label="
                secretReplacing
                  ? t('cacheAlertSecretReplacing')
                  : t('cacheAlertSecretKeeping')
              "
            />
            <UTooltip :text="t('cacheAlertSecretHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('cacheAlertSecretHelp')"
              />
            </UTooltip>
          </span>
          <UInput
            v-model="secretInput"
            size="sm"
            type="password"
            :placeholder="t('cacheAlertSecretPlaceholder')"
          />
        </div>
      </div>

      <p v-if="cacheAlert" class="m-0 text-[11px] leading-relaxed text-muted">
        {{ t('cacheAlertHint') }}
      </p>
    </SettingsCard>
  </section>
</template>
