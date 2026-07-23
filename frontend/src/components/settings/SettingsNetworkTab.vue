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
  <section class="grid gap-4">
    <SettingsCard>
      <template #header>
        <div>
          <h3
            class="m-0 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            {{ t('relayIpWhitelist') }}
          </h3>
        </div>
        <UButton
          size="sm"
          :loading="busy"
          @click="$emit('saveRelayIpWhitelist')"
          >{{ t('save') }}</UButton
        >
      </template>
      <div class="grid gap-3">
        <div class="grid gap-3">
          <UCollapsible>
            <template #default="{ open }">
              <UButton
                color="neutral"
                variant="subtle"
                :label="t('allowedCidrs')"
                :trailing-icon="
                  open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'
                "
                block
              />
            </template>
            <template #content>
              <div class="grid gap-2">
                <UTextarea
                  id="settings-allowed-cidrs"
                  v-model="relayIpWhitelist.allowed_cidrs_text"
                  autoresize
                  :rows="6"
                  class="w-full font-mono"
                  name="settings-allowed-cidrs"
                  :placeholder="t('allowedCidrsPlaceholder')"
                />
                <div class="text-xs text-dimmed">
                  {{ t('allowedCidrsHelp') }}
                </div>
              </div>
            </template>
          </UCollapsible>

          <UCollapsible>
            <template #default="{ open }">
              <UButton
                color="neutral"
                variant="subtle"
                :label="t('trustedProxyCidrs')"
                :trailing-icon="
                  open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'
                "
                block
              />
            </template>
            <template #content>
              <div class="grid gap-2">
                <UTextarea
                  id="settings-trusted-proxy-cidrs"
                  v-model="relayIpWhitelist.trusted_proxy_cidrs_text"
                  autoresize
                  :rows="6"
                  class="w-full font-mono"
                  name="settings-trusted-proxy-cidrs"
                  :placeholder="t('trustedProxyCidrsPlaceholder')"
                />
                <div class="text-xs text-dimmed">
                  {{ t('trustedProxyCidrsHelp') }}
                </div>
              </div>
            </template>
          </UCollapsible>
        </div>
      </div>
    </SettingsCard>
  </section>
</template>
