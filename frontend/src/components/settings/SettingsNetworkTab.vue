<script setup lang="ts">
import SettingsCard from './SettingsCard.vue'

defineProps<{
  busy: boolean
  t: TranslateFn
}>()

const relayIpWhitelist = defineModel<{
  allowed_cidrs_text: string
  trusted_proxy_cidrs_text: string
}>('relayIpWhitelist', { required: true })

defineEmits<{
  saveRelayIpWhitelist: []
}>()
</script>

<template>
  <section class="grid gap-3">
    <SettingsCard>
      <template #header>
        <h3
          class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
        >
          <UIcon name="i-lucide-shield" class="h-3.5 w-3.5 text-muted" />
          {{ t('relayIpWhitelist') }}
        </h3>
        <UButton
          size="sm"
          icon="i-lucide-save"
          :loading="busy"
          @click="$emit('saveRelayIpWhitelist')"
          >{{ t('save') }}</UButton
        >
      </template>
      <div class="grid gap-3 md:grid-cols-2">
        <div class="grid gap-1.5">
          <div class="flex items-center gap-1">
            <label
              class="text-xs font-medium text-muted"
              for="settings-allowed-cidrs"
            >
              {{ t('allowedCidrs') }}
            </label>
            <UTooltip :text="t('allowedCidrsHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('allowedCidrsHelp')"
              />
            </UTooltip>
          </div>
          <UTextarea
            id="settings-allowed-cidrs"
            v-model="relayIpWhitelist.allowed_cidrs_text"
            autoresize
            :rows="5"
            class="w-full font-mono text-xs"
            name="settings-allowed-cidrs"
            :placeholder="t('allowedCidrsPlaceholder')"
          />
          <p class="m-0 text-[11px] leading-relaxed text-muted">
            {{ t('allowedCidrsHelp') }}
          </p>
        </div>

        <div class="grid gap-1.5">
          <div class="flex items-center gap-1">
            <label
              class="text-xs font-medium text-muted"
              for="settings-trusted-proxy-cidrs"
            >
              {{ t('trustedProxyCidrs') }}
            </label>
            <UTooltip :text="t('trustedProxyCidrsHelp')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('trustedProxyCidrsHelp')"
              />
            </UTooltip>
          </div>
          <UTextarea
            id="settings-trusted-proxy-cidrs"
            v-model="relayIpWhitelist.trusted_proxy_cidrs_text"
            autoresize
            :rows="5"
            class="w-full font-mono text-xs"
            name="settings-trusted-proxy-cidrs"
            :placeholder="t('trustedProxyCidrsPlaceholder')"
          />
          <p class="m-0 text-[11px] leading-relaxed text-muted">
            {{ t('trustedProxyCidrsHelp') }}
          </p>
        </div>
      </div>
    </SettingsCard>
  </section>
</template>
