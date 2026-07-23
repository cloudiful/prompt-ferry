<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref } from 'vue'
import type {
  AppliedReplacementSchema,
  RedactionFindingSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
} from '@/generated/admin-api'
import type { RedactionWorkspaceView } from '@/models/redaction'
import FlatSection from '@/components/shared/FlatSection.vue'

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
              id="redaction-preview-input"
              v-model="previewText"
              :rows="7"
              class="w-full font-mono text-[13px] leading-6"
              name="redaction-preview-input"
            />
          </label>
          <div
            class="grid min-w-0 gap-2 max-[767px]:hidden"
            :class="{ 'max-[767px]:grid': activePreviewPane === 'output' }"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="text-xs text-muted">{{ t('redactionOutput') }}</span>
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
              <UTextarea
                v-if="previewResult"
                id="redaction-preview-output"
                :model-value="previewResult.redacted_text"
                :rows="7"
                class="h-full min-h-[13rem] w-full font-mono text-[13px] leading-6"
                name="redaction-preview-output"
                readonly
              />
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
          >
            <template #empty>{{ t('noFindings') }}</template>
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
          >
            <template #empty>{{ t('noReplacements') }}</template>
          </UTable>
        </section>
      </div>
    </FlatSection>
  </div>
</template>
