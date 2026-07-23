<template>
  <UApp :locale="uiLocale">
    <RouterView />
  </UApp>
</template>

<script setup lang="ts">
import { en, zh_cn } from '@nuxt/ui/locale'
import { computed } from 'vue'
import { locale } from './composables/useLocale'

const uiLocale = computed(() => (locale.value === 'en-US' ? en : zh_cn))
</script>
