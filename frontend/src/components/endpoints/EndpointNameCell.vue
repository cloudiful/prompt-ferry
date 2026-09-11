<script setup lang="ts">
import ProviderIcon from '@/components/providers/ProviderIcon.vue'

defineProps<{
  name: string
  baseUrl: string
  provider: string
}>()
</script>

<template>
  <div class="flex min-w-0 max-w-40 items-center gap-1.5">
    <ProviderIcon :provider="provider" size="sm" />
    <div class="min-w-0">
      <div class="truncate font-semibold text-highlighted">{{ name }}</div>
      <div class="truncate text-xs text-muted">{{ baseUrl }}</div>
    </div>
  </div>
</template>
