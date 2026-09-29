<script setup lang="ts">
import { computed } from 'vue'
import { normalizeServiceTier } from '@/admin-mappers'
import SettingsFieldRow from '@/components/shared/SettingsFieldRow.vue'

defineProps<{
  t: TranslateFn
  inputId?: string
}>()

const model = defineModel<string | null>({ required: true })

// Issue #637: free-form override; blank/whitespace maps to inherit (`null`)
// so the request never injects an empty tier.
const value = computed({
  get: () => model.value ?? '',
  set: (next: string) => {
    model.value = normalizeServiceTier(next)
  },
})
</script>

<template>
  <SettingsFieldRow
    :label="t('serviceTier')"
    :hint="t('serviceTierHint')"
    :for-id="inputId"
  >
    <UInput
      :id="inputId"
      v-model="value"
      class="w-full"
      :placeholder="t('proxyInherit')"
    />
  </SettingsFieldRow>
</template>
