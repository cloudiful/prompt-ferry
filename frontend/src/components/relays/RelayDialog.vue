<script setup lang="ts">
import type { ManagedRelay } from '@/generated/admin-api'
import type { Option } from '@/models'
import type { RelayForm, RelaySecretAction } from '@/admin-mappers/forms/relay'

defineProps<{
  bridgeOptions: Option[]
  busy: boolean
  currentRelay: ManagedRelay | null
  header: string
  secretActionOptions: Option<RelaySecretAction>[]
  t: TranslateFn
  tlsOptions: Option[]
}>()

const open = defineModel<boolean>('open', { required: true })
const form = defineModel<RelayForm>('form', { required: true })

defineEmits<{ save: [] }>()

function formatTimestamp(value?: string | null): string {
  if (!value) return '-'
  const date = new Date(value)
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString()
}
</script>

<template>
  <UModal
    v-model:open="open"
    :title="header"
    :ui="{ content: 'sm:max-w-5xl', body: 'max-h-[80vh] overflow-y-auto' }"
  >
    <template #body>
      <div class="grid gap-4">
        <div class="grid gap-2 md:grid-cols-2">
          <UFormField :label="t('name')">
            <UInput
              v-model="form.name"
              :placeholder="t('relayNameHint')"
              class="w-full"
            />
          </UFormField>
          <UFormField :label="t('relayUrl')">
            <UInput
              v-model="form.relay_url"
              placeholder="wss://relay.example.com/ws/worker"
              class="w-full"
            />
          </UFormField>
          <UFormField :label="t('relayTlsMode')">
            <USelect
              v-model="form.tls_mode"
              :items="tlsOptions"
              class="w-full"
            />
          </UFormField>
          <UFormField :label="t('relayBridgeMode')">
            <USelect
              v-model="form.bridge_encryption_mode"
              :items="bridgeOptions"
              class="w-full"
            />
          </UFormField>
        </div>

        <UCheckbox v-model="form.enabled" :label="t('relayEnabled')" />

        <div class="grid gap-4 md:grid-cols-2">
          <UCard>
            <template #header>
              <div class="flex items-center justify-between gap-2">
                <span class="text-sm font-medium">{{ t('relayCaPem') }}</span>
                <USelect
                  v-if="form.relay_id"
                  v-model="form.relay_ca_action"
                  :items="secretActionOptions"
                  class="w-36"
                />
              </div>
            </template>
            <UTextarea
              v-model="form.relay_ca_pem"
              :rows="5"
              autoresize
              class="w-full"
            />
          </UCard>

          <UCard v-if="form.bridge_encryption_mode === 'required'">
            <template #header>
              <div class="flex items-center justify-between gap-2">
                <span class="text-sm font-medium">{{
                  t('relayBridgeKey')
                }}</span>
                <USelect
                  v-if="form.relay_id"
                  v-model="form.bridge_key_action"
                  :items="secretActionOptions"
                  class="w-36"
                />
              </div>
            </template>
            <UTextarea
              v-model="form.bridge_encryption_key"
              :rows="5"
              autoresize
              class="w-full"
            />
          </UCard>
        </div>

        <div v-if="form.tls_mode === 'mtls'" class="grid gap-4 md:grid-cols-2">
          <UCard>
            <template #header>
              <div class="flex items-center justify-between gap-2">
                <span class="text-sm font-medium">{{
                  t('relayClientCertPem')
                }}</span>
                <USelect
                  v-if="form.relay_id"
                  v-model="form.client_cert_action"
                  :items="secretActionOptions"
                  class="w-36"
                />
              </div>
            </template>
            <UTextarea
              v-model="form.client_cert_pem"
              :rows="6"
              autoresize
              class="w-full"
            />
          </UCard>
          <UCard>
            <template #header>
              <div class="flex items-center justify-between gap-2">
                <span class="text-sm font-medium">{{
                  t('relayClientKeyPem')
                }}</span>
                <USelect
                  v-if="form.relay_id"
                  v-model="form.client_key_action"
                  :items="secretActionOptions"
                  class="w-36"
                />
              </div>
            </template>
            <UTextarea
              v-model="form.client_key_pem"
              :rows="6"
              autoresize
              class="w-full"
            />
          </UCard>
        </div>

        <div v-if="form.relay_id" class="grid gap-1 text-xs text-muted">
          <span
            >{{ t('relayLastConnectedAt') }}:
            {{ formatTimestamp(currentRelay?.last_connected_at) }}</span
          >
          <span
            >{{ t('relayLastDisconnectedAt') }}:
            {{ formatTimestamp(currentRelay?.last_disconnected_at) }}</span
          >
        </div>
      </div>
    </template>

    <template #footer>
      <div class="flex w-full justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          :label="t('cancel')"
          @click="
            () => {
              open = false
            }
          "
        />
        <UButton :loading="busy" :label="t('save')" @click="$emit('save')" />
      </div>
    </template>
  </UModal>
</template>
