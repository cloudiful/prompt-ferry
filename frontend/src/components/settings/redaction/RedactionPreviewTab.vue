<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref, watch } from 'vue'
import type {
  AppliedReplacementSchema,
  RedactionFindingSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
} from '@/generated/admin-api'
import type { RedactionWorkspaceView } from '@/models/redaction'
import FlatSection from '@/components/shared/FlatSection.vue'
import { copyText } from '@/composables/useClipboard'

const props = defineProps<{
  busy: boolean
  t: TranslateFn
  workspace: RedactionWorkspaceView
}>()

const findingColumns = computed<TableColumn<RedactionFindingSchema>[]>(() => [
  { accessorKey: 'kind', header: props.t('type') },
  { accessorKey: 'source', header: props.t('sourceField') },
  { accessorKey: 'confidence', header: props.t('scoreField') },
  { accessorKey: 'match_text', header: props.t('pattern') },
])
const replacementColumns = computed<TableColumn<AppliedReplacementSchema>[]>(
  () => [
    { accessorKey: 'kind', header: props.t('type') },
    { accessorKey: 'replacement', header: props.t('replacementField') },
    { accessorKey: 'display_value', header: props.t('hintField') },
    { accessorKey: 'strategy', header: props.t('strategyField') },
  ],
)

const previewText = defineModel<string>('previewText', { required: true })
const previewInputKind = defineModel<RedactionInputKindSchema>(
  'previewInputKind',
  {
    required: true,
  },
)
const previewResult = defineModel<RedactionPreviewSchema | null>(
  'previewResult',
  {
    required: true,
  },
)
const activePreviewPane = ref<'input' | 'output'>('input')
const activeFindingIndex = ref<number | null>(null)
const copyState = ref<'idle' | 'copied'>('idle')

const selectedFinding = computed<RedactionFindingSchema | null>(() => {
  if (activeFindingIndex.value === null) return null
  return previewResult.value?.findings[activeFindingIndex.value] ?? null
})

const highlightedOutputSegments = computed(() => {
  const text = previewResult.value?.redacted_text ?? ''
  if (!selectedFinding.value) {
    return [{ text, highlighted: false }]
  }
  const { start, end } = selectedFinding.value
  if (
    typeof start !== 'number' ||
    typeof end !== 'number' ||
    start < 0 ||
    end <= start ||
    start >= text.length
  ) {
    return [{ text, highlighted: false }]
  }
  const clampedEnd = Math.min(end, text.length)
  const before = text.slice(0, start)
  const hit = text.slice(start, clampedEnd)
  const after = text.slice(clampedEnd)
  const segments: Array<{ text: string; highlighted: boolean }> = []
  if (before) segments.push({ text: before, highlighted: false })
  if (hit) segments.push({ text: hit, highlighted: true })
  if (after) segments.push({ text: after, highlighted: false })
  return segments
})

const highlightedInputSegments = computed(() => {
  const text = previewText.value
  if (!selectedFinding.value) {
    return [{ text, highlighted: false }]
  }
  const { match_text, start } = selectedFinding.value
  if (typeof start !== 'number' || !match_text || !text.includes(match_text)) {
    return [{ text, highlighted: false }]
  }
  const hitIndex = text.indexOf(match_text, start)
  if (hitIndex < 0) {
    return [{ text, highlighted: false }]
  }
  const before = text.slice(0, hitIndex)
  const after = text.slice(hitIndex + match_text.length)
  const segments: Array<{ text: string; highlighted: boolean }> = []
  if (before) segments.push({ text: before, highlighted: false })
  segments.push({ text: match_text, highlighted: true })
  if (after) segments.push({ text: after, highlighted: false })
  return segments
})

watch(
  () => previewResult.value?.findings ?? null,
  () => {
    activeFindingIndex.value = null
    copyState.value = 'idle'
  },
)

function selectFinding(index: number): void {
  activeFindingIndex.value = index
  activePreviewPane.value = 'output'
}

async function copyOutput(): Promise<void> {
  const text = previewResult.value?.redacted_text ?? ''
  if (!text) return
  await copyText(text)
  copyState.value = 'copied'
}

watch(copyState, (state) => {
  if (state !== 'copied') return
  const timer = setTimeout(() => {
    copyState.value = 'idle'
  }, 1500)
  return () => clearTimeout(timer)
})

defineEmits<{
  runPreview: []
}>()
</script>

<template>
  <div class="grid gap-3">
    <FlatSection :title="t('redactionPreview')">
      <template #actions>
        <UButton size="sm" :loading="busy" @click="$emit('runPreview')">
          <UIcon name="i-lucide-eye" class="h-4 w-4" />
          {{ t('preview') }}
        </UButton>
      </template>
      <div class="grid gap-3">
        <label class="grid gap-2 md:max-w-64">
          <span class="text-xs text-muted">{{ t('inputKind') }}</span>
          <USelect
            v-model="previewInputKind"
            :aria-label="t('inputKind')"
            class="w-full"
            id="redaction-input-kind"
            size="sm"
            :items="workspace.input_kind_options"
            label-key="label"
            value-key="value"
          />
        </label>

        <div v-if="previewResult" class="flex flex-wrap gap-2">
          <UBadge
            v-for="stat in workspace.preview_stats"
            :key="stat.label"
            :label="`${stat.label} ${stat.value}`"
          />
        </div>

        <div class="hidden gap-2 max-[767px]:flex">
          <button
            type="button"
            class="flex-1 rounded-full border border-default bg-default px-2.5 py-1 text-[0.72rem] leading-[1.1] text-muted"
            :class="
              activePreviewPane === 'input'
                ? 'border-primary bg-elevated text-primary'
                : ''
            "
            @click="
              () => {
                activePreviewPane = 'input'
              }
            "
          >
            {{ t('redactionInput') }}
          </button>
          <button
            type="button"
            class="flex-1 rounded-full border border-default bg-default px-2.5 py-1 text-[0.72rem] leading-[1.1] text-muted"
            :class="
              activePreviewPane === 'output'
                ? 'border-primary bg-elevated text-primary'
                : ''
            "
            @click="
              () => {
                activePreviewPane = 'output'
              }
            "
          >
            {{ t('redactionOutput') }}
          </button>
        </div>

        <div class="grid items-start gap-3 md:grid-cols-2">
          <label
            class="grid min-w-0 gap-2 max-[767px]:hidden"
            :class="{ 'max-[767px]:grid': activePreviewPane === 'input' }"
          >
            <span class="text-xs text-muted">{{ t('redactionInput') }}</span>
            <UTextarea
              v-if="!selectedFinding"
              id="redaction-preview-input"
              v-model="previewText"
              :rows="7"
              class="w-full font-mono text-[13px] leading-6"
              name="redaction-preview-input"
            />
            <div
              v-else
              id="redaction-preview-input"
              class="min-h-[13rem] w-full overflow-auto whitespace-pre-wrap rounded-md border border-default bg-default px-3 py-2 font-mono text-[13px] leading-6"
              name="redaction-preview-input"
            >
              <span
                v-for="(segment, index) in highlightedInputSegments"
                :key="`in-${index}`"
                :class="
                  segment.highlighted
                    ? 'rounded bg-warning/20 px-0.5 text-warning'
                    : ''
                "
                >{{ segment.text }}</span
              >
            </div>
          </label>
          <div
            class="grid min-w-0 gap-2 max-[767px]:hidden"
            :class="{ 'max-[767px]:grid': activePreviewPane === 'output' }"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="text-xs text-muted">{{ t('redactionOutput') }}</span>
              <UButton
                v-if="previewResult"
                size="xs"
                color="neutral"
                variant="ghost"
                :aria-label="t('copyOutput')"
                @click="copyOutput"
              >
                <UIcon
                  :name="
                    copyState === 'copied'
                      ? 'i-lucide-clipboard-check'
                      : 'i-lucide-copy'
                  "
                  class="h-3.5 w-3.5"
                />
                {{ copyState === 'copied' ? t('copied') : t('copy') }}
              </UButton>
            </div>
            <div
              v-if="
                previewResult?.stats.llm_request_failed &&
                previewResult.stats.llm_error
              "
              class="rounded border border-error bg-error/10 px-3 py-2 text-[0.75rem] text-error"
            >
              {{ previewResult.stats.llm_error }}
            </div>
            <div class="min-h-full">
              <div
                v-if="previewResult && !selectedFinding"
                id="redaction-preview-output"
                class="min-h-[13rem] w-full overflow-auto whitespace-pre-wrap rounded-md border border-default bg-default px-3 py-2 font-mono text-[13px] leading-6"
                name="redaction-preview-output"
              >
                {{ previewResult.redacted_text }}
              </div>
              <div
                v-else-if="previewResult"
                id="redaction-preview-output"
                class="min-h-[13rem] w-full overflow-auto whitespace-pre-wrap rounded-md border border-default bg-default px-3 py-2 font-mono text-[13px] leading-6"
                name="redaction-preview-output"
              >
                <span
                  v-for="(segment, index) in highlightedOutputSegments"
                  :key="`out-${index}`"
                  :class="
                    segment.highlighted
                      ? 'rounded bg-warning/20 px-0.5 text-warning'
                      : ''
                  "
                  >{{ segment.text }}</span
                >
              </div>
              <div
                v-else
                class="flex min-h-[11rem] items-center justify-center rounded border border-dashed border-default bg-muted p-6 text-center text-[0.75rem] text-muted"
              >
                {{ t('preview') }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </FlatSection>

    <FlatSection :title="t('redactionPreviewDetails')">
      <div class="grid gap-4">
        <section class="grid gap-2">
          <h3
            class="m-0 text-[0.92rem] leading-[1.3] font-semibold text-highlighted"
          >
            {{ t('findings') }}
          </h3>
          <UTable
            :data="previewResult?.findings ?? []"
            :columns="findingColumns"
            class="min-w-0"
            :ui="{
              th: 'whitespace-nowrap',
              tr: 'cursor-pointer',
            }"
            @select="(_event, row) => selectFinding(row.index)"
          >
            <template #empty>{{ t('noFindings') }}</template>
            <template #match_text-cell="{ row }">
              <span
                class="rounded px-0.5"
                :class="
                  activeFindingIndex === row.index
                    ? 'bg-primary/15 text-primary'
                    : ''
                "
              >
                {{ row.original.match_text }}
              </span>
            </template>
          </UTable>
        </section>

        <section class="grid gap-2">
          <h3
            class="m-0 text-[0.92rem] leading-[1.3] font-semibold text-highlighted"
          >
            {{ t('replacements') }}
          </h3>
          <UTable
            :data="previewResult?.applied_replacements ?? []"
            :columns="replacementColumns"
            class="min-w-0"
            :ui="{ th: 'whitespace-nowrap' }"
          >
            <template #empty>{{ t('noReplacements') }}</template>
          </UTable>
        </section>
      </div>
    </FlatSection>
  </div>
</template>
