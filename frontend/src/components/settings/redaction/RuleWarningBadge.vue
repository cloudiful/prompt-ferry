<script setup lang="ts">
import type { CustomStringRuleWarning } from '@/redaction-rules'
import type { RedactionCustomStringRuleRowSchema } from '@/generated/admin-api'

const props = defineProps<{
  row: { original: RedactionCustomStringRuleRowSchema }
  warning: CustomStringRuleWarning
  t: TranslateFn
}>()

function badgeText(): string {
  if (props.warning.invalidRegex) return props.t('ruleInvalidRegex')
  if (props.warning.duplicate) return props.t('ruleDuplicateHint')
  return props.t('ruleConflictHint')
}

function badgeClass(): string {
  if (props.warning.invalidRegex) {
    return 'border-error bg-error/10 text-error'
  }
  return 'border-warning bg-warning/10 text-warning'
}

function badgeIcon(): string {
  return props.warning.invalidRegex
    ? 'i-lucide-circle-x'
    : 'i-lucide-circle-alert'
}
</script>

<template>
  <UTooltip :text="badgeText()">
    <div
      class="inline-flex w-fit items-center gap-1 rounded-md border px-1.5 py-0.5 text-[0.7rem] leading-[1.1]"
      :class="badgeClass()"
    >
      <UIcon :name="badgeIcon()" class="h-3 w-3" />
      <span>{{ badgeText() }}</span>
    </div>
  </UTooltip>
</template>
