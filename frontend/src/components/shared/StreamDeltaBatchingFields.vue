<script setup lang="ts">
type StreamDeltaBatchingEditable = {
  flush_window_ms: number
  max_buffer_chars: number
  max_buffer_bytes: number
  flush_on_line_break: boolean
  flush_on_sentence_end: boolean
}

withDefaults(
  defineProps<{
    compact?: boolean
    t: TranslateFn
  }>(),
  {
    compact: false,
  },
)

const form = defineModel<StreamDeltaBatchingEditable>('form', {
  required: true,
})
</script>

<template>
  <div v-if="compact" class="grid gap-2">
    <div class="flex flex-wrap items-center gap-2">
      <UInputNumber
        v-model="form.flush_window_ms"
        size="sm"
        :min="1"
        :max="1000"
        :use-grouping="false"
      />
      <span class="text-xs text-muted">{{ t('flushWindowMs') }}</span>
      <UInputNumber
        v-model="form.max_buffer_chars"
        size="sm"
        :min="1"
        :max="8192"
        :use-grouping="false"
      />
      <span class="text-xs text-muted">{{ t('maxBufferChars') }}</span>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <UInputNumber
        v-model="form.max_buffer_bytes"
        size="sm"
        :min="1"
        :max="65536"
        :use-grouping="false"
      />
      <span class="text-xs text-muted">{{ t('maxBufferBytes') }}</span>
      <label
        class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-default"
        ><UCheckbox v-model="form.flush_on_line_break" />{{
          t('flushOnLineBreak')
        }}</label
      >
      <label
        class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-default"
        ><UCheckbox v-model="form.flush_on_sentence_end" />{{
          t('flushOnSentenceEnd')
        }}</label
      >
    </div>
  </div>
  <div v-else class="grid gap-2">
    <div class="grid items-end gap-3 md:grid-cols-[8rem_auto]">
      <div class="grid gap-1.5">
        <label class="text-xs text-muted">{{ t('flushWindowMs') }}</label>
        <UInputNumber
          v-model="form.flush_window_ms"
          size="sm"
          :min="1"
          :max="1000"
          :use-grouping="false"
        />
      </div>
    </div>

    <UCollapsible default-open class="mt-1">
      <template #default="{ open }">
        <UButton
          color="neutral"
          variant="subtle"
          :label="t('streamBuffering')"
          :trailing-icon="
            open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'
          "
          block
        />
      </template>
      <template #content>
        <div class="grid gap-3 md:grid-cols-2">
          <div class="grid gap-2">
            <label class="text-xs text-muted">{{ t('maxBufferChars') }}</label>
            <UInputNumber
              v-model="form.max_buffer_chars"
              size="sm"
              :min="1"
              :max="8192"
              :use-grouping="false"
            />
          </div>
          <div class="grid gap-2">
            <label class="text-xs text-muted">{{ t('maxBufferBytes') }}</label>
            <UInputNumber
              v-model="form.max_buffer_bytes"
              size="sm"
              :min="1"
              :max="65536"
              :use-grouping="false"
            />
          </div>
          <label
            class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-default"
            ><UCheckbox v-model="form.flush_on_line_break" />{{
              t('flushOnLineBreak')
            }}</label
          >
          <label
            class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-default"
            ><UCheckbox v-model="form.flush_on_sentence_end" />{{
              t('flushOnSentenceEnd')
            }}</label
          >
        </div>
      </template>
    </UCollapsible>
  </div>
</template>
