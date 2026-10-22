<script setup lang="ts">
import { computed } from 'vue'
import { useEndpointModels } from '@/composables/useEndpointModels'
import type { NativeApi } from '@/generated/admin-api'
import type { TranslateFn } from '@/i18n'

const props = defineProps<{
  disabled?: boolean
  endpointId: string | null
  // Issue #1032: the effective target protocol scopes the suggestion catalog.
  // Omitted (or `auto`) keeps the endpoint's own protocol semantics, so an
  // `auto` endpoint may return the union of its catalogs.
  nativeApi?: NativeApi | null
  placeholder?: string
  t: TranslateFn
}>()

const value = defineModel<string>({ required: true })

const { errorMessage, loading, models } = useEndpointModels(
  computed(() => props.endpointId),
  computed(() => props.nativeApi ?? null),
)
</script>

<template>
  <div class="grid gap-1">
    <UInputMenu
      v-model="value"
      mode="autocomplete"
      create-item
      clear
      :items="models"
      :loading="loading"
      :disabled="disabled"
      :placeholder="placeholder"
      class="w-full"
      @clear="value = ''"
    />
    <p v-if="errorMessage" class="text-xs text-error">
      {{ t('upstreamModelSuggestionsFailed') }}
    </p>
  </div>
</template>
