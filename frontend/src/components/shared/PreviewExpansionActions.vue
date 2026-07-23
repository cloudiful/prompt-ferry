<script setup lang="ts">
withDefaults(
  defineProps<{
    allLabel: string
    buttonsClass?: string
    collapseLabel: string
    expanded: boolean
    hasMore: boolean
    moreLabel: string
    truncatedLabel: string
  }>(),
  {
    buttonsClass: 'flex flex-wrap gap-2',
  },
)

defineEmits<{
  all: []
  collapse: []
  more: []
}>()
</script>

<template>
  <div v-if="hasMore || expanded" class="grid gap-2">
    <div v-if="hasMore" class="text-xs text-dimmed">
      {{ truncatedLabel }}
    </div>
    <div :class="buttonsClass">
      <UButton
        v-if="hasMore"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="$emit('more')"
      >
        {{ moreLabel }}
      </UButton>
      <UButton
        v-if="hasMore"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="$emit('all')"
      >
        {{ allLabel }}
      </UButton>
      <UButton
        v-if="expanded"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="$emit('collapse')"
      >
        {{ collapseLabel }}
      </UButton>
    </div>
  </div>
</template>
