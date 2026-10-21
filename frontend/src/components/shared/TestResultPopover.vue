<script setup lang="ts">
import { computed } from 'vue'

const props = withDefaults(
  defineProps<{
    message: string
    severity: 'success' | 'warning' | 'error' | null
    detailLabel?: string
    interactive?: boolean
  }>(),
  { detailLabel: '', interactive: false },
)

const emit = defineEmits<{
  select: []
}>()

// success -> green check, warning -> amber triangle, error -> red cross,
// idle -> gray dash. The popover trigger stays a button so screen readers
// announce it as a control; the icon is purely visual and the accessible
// label carries the full test message.
const iconName = computed(() => {
  if (props.severity === 'success') return 'i-lucide-check'
  if (props.severity === 'warning') return 'i-lucide-alert-triangle'
  if (props.severity === 'error') return 'i-lucide-x'
  return 'i-lucide-minus'
})

const iconTone = computed(() => {
  if (props.severity === 'success') return 'text-success'
  if (props.severity === 'warning') return 'text-warning'
  if (props.severity === 'error') return 'text-error'
  return 'text-muted'
})
</script>

<template>
  <UPopover
    mode="hover"
    :enable-touch="true"
    :content="{
      side: 'bottom',
      align: 'start',
      sideOffset: 6,
      collisionPadding: 8,
    }"
  >
    <UButton
      type="button"
      size="xs"
      color="neutral"
      variant="ghost"
      square
      :aria-label="message"
      :title="message"
      class="rounded-full"
      @click="interactive && emit('select')"
    >
      <UIcon :name="iconName" :class="['h-4 w-4', iconTone]" />
      <span v-if="detailLabel" class="text-xs text-muted">{{
        detailLabel
      }}</span>
    </UButton>
    <template #content>
      <div
        class="max-h-[50vh] max-w-[min(22rem,calc(100vw-2rem))] overflow-auto break-words whitespace-pre-wrap p-3 text-sm leading-relaxed [overflow-wrap:anywhere]"
      >
        {{ message }}
      </div>
    </template>
  </UPopover>
</template>
