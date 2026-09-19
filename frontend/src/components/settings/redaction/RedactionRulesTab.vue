<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref } from 'vue'
import type {
  RedactionConfigSchema,
  RedactionCustomStringRuleRowSchema,
} from '@/generated/admin-api'
import TablePagination from '@/components/shared/TablePagination.vue'
import DeleteRuleConfirmDialog from '@/components/settings/redaction/DeleteRuleConfirmDialog.vue'
import RuleWarningBadge from '@/components/settings/redaction/RuleWarningBadge.vue'
import type { MessageKey } from '@/i18n'
import type { RedactionWorkspaceView } from '@/models/redaction'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'
import { useRedactionStore } from '@/stores/redaction'
import { storeToRefs } from 'pinia'

const props = defineProps<{
  t: TranslateFn
  workspace: RedactionWorkspaceView
}>()

const columns = computed<TableColumn<RedactionCustomStringRuleRowSchema>[]>(
  () => [
    { id: 'pattern', header: props.t('pattern') },
    { id: 'matchType', header: props.t('matchType') },
    { id: 'scope', header: props.t('scopeField') },
    { id: 'createdAt', header: props.t('createdAt') },
    { id: 'updatedAt', header: props.t('updatedAt') },
    { id: 'actions' },
  ],
)

type MatchType = 'exact' | 'contains' | 'regex'
type RuleScope = 'text' | 'line'

function isMatchType(value: string): value is MatchType {
  return value === 'exact' || value === 'contains' || value === 'regex'
}

function isRuleScope(value: string): value is RuleScope {
  return value === 'text' || value === 'line'
}

function normalizeMatchType(value: string): MatchType | undefined {
  return isMatchType(value) ? value : undefined
}

function normalizeRuleScope(value: string): RuleScope | undefined {
  return isRuleScope(value) ? value : undefined
}

function matchTypeTooltipKey(value: string): MessageKey {
  if (value === 'exact') return 'matchTypeExactHint'
  if (value === 'contains') return 'matchTypeContainsHint'
  if (value === 'regex') return 'matchTypeRegexHint'
  return 'matchTypeHint'
}

function scopeTooltipKey(value: string): MessageKey {
  if (value === 'text') return 'scopeTextHint'
  if (value === 'line') return 'scopeLineHint'
  return 'scopeFieldHint'
}

const matchTypeOptions = computed(() =>
  props.workspace.match_type_options.flatMap((option) => {
    const value = option.value
    return isMatchType(value) ? [{ ...option, value }] : []
  }),
)
const scopeOptions = computed(() =>
  props.workspace.scope_options.flatMap((option) => {
    const value = option.value
    return isRuleScope(value) ? [{ ...option, value }] : []
  }),
)

const config = defineModel<RedactionConfigSchema>('config', { required: true })
const redactionStore = useRedactionStore()
const {
  customStringFirst,
  customStringRows,
  customStringSearch,
  customStringTotal,
  customStringWarnings,
  customStrings,
  isDirty,
} = storeToRefs(redactionStore)

const customStringSearchModel = computed({
  get: () => customStringSearch.value,
  set: (value: string) => {
    void redactionStore.setCustomStringSearch(value)
  },
})

const pendingDeleteIndex = ref<number | null>(null)
const pendingDeleteRule = computed(() => {
  if (pendingDeleteIndex.value === null) return null
  const rule = config.value.custom_strings[pendingDeleteIndex.value]
  if (!rule) return null
  return { index: pendingDeleteIndex.value, pattern: rule.pattern }
})

function warningForIndex(index: number) {
  return (
    customStringWarnings.value[index] ?? {
      duplicate: false,
      conflict: false,
      invalidRegex: false,
    }
  )
}

function hasWarning(index: number): boolean {
  const warning = warningForIndex(index)
  return warning.invalidRegex || warning.duplicate || warning.conflict
}

function requestDeleteRule(index: number): void {
  pendingDeleteIndex.value = index
}

function cancelDeleteRule(): void {
  pendingDeleteIndex.value = null
}

function confirmDeleteRule(): void {
  if (pendingDeleteIndex.value === null) return
  redactionStore.removeCustomStringRule(pendingDeleteIndex.value)
  pendingDeleteIndex.value = null
}

function onCustomStringPage(event: TablePageChange): void {
  void redactionStore.setCustomStringPage(event.first, event.rows)
}

function formatDateTime(value: string | null | undefined): string {
  if (!value) return '-'
  const date = new Date(value)
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString()
}
</script>

<template>
  <div class="grid gap-3">
    <section class="grid gap-3">
      <div class="flex flex-wrap items-center gap-2">
        <div
          v-if="isDirty"
          class="inline-flex items-center gap-1 rounded-md border border-warning bg-warning/10 px-2 py-1 text-[0.72rem] font-medium text-warning"
        >
          <UIcon name="i-lucide-circle-alert" class="h-3.5 w-3.5" />
          {{ t('ruleDirtyBadge') }}
        </div>
        <div class="flex flex-wrap gap-2">
          <label
            v-for="rule in workspace.rule_options"
            :key="rule.key"
            class="inline-flex min-w-0 items-center gap-2 rounded-md border border-default bg-default px-2 py-1.5"
          >
            <UCheckbox
              v-model="config.rules[rule.key]"
              :id="`redaction-rule-${rule.key}`"
            />
            <span
              class="min-w-0 text-[0.95rem] leading-[1.2] font-semibold text-highlighted"
            >
              {{ rule.label }}
            </span>
          </label>
        </div>
      </div>
    </section>

    <section class="grid gap-3">
      <div class="flex min-w-0 flex-1 items-center justify-between gap-3">
        <h2
          class="m-0 whitespace-nowrap text-[0.98rem] leading-[1.3] font-semibold text-highlighted"
        >
          {{ t('redactionAdvancedRules') }}
        </h2>
        <div class="flex min-w-0 shrink-0 items-center gap-2 whitespace-nowrap">
          <UInput
            v-model="customStringSearchModel"
            class="w-48"
            size="sm"
            :placeholder="t('searchPattern')"
          />
          <UButton
            size="sm"
            color="neutral"
            variant="outline"
            @click="redactionStore.addCustomStringRule()"
          >
            <UIcon name="i-lucide-plus" class="h-4 w-4" />
            {{ t('addRule') }}
          </UButton>
        </div>
      </div>
      <UTable
        :data="customStrings"
        :columns="columns"
        class="min-w-0"
        :ui="{ th: 'whitespace-nowrap' }"
      >
        <template #empty>{{ t('noCustomStrings') }}</template>
        <template #pattern-cell="{ row }">
          <div class="grid min-w-0 gap-1">
            <div class="flex min-w-0 items-center gap-1">
              <UInput
                :model-value="row.original.pattern"
                :type="
                  redactionStore.isCustomStringRevealed(
                    row.original.array_index,
                  )
                    ? 'text'
                    : 'password'
                "
                :class="[
                  'min-w-0 flex-1',
                  warningForIndex(row.original.array_index).invalidRegex
                    ? 'ring-1 ring-error'
                    : '',
                ]"
                :aria-invalid="
                  warningForIndex(row.original.array_index).invalidRegex
                "
                name="redaction-custom-pattern"
                size="sm"
                @update:model-value="
                  redactionStore.updateCustomStringRule(
                    row.original.array_index,
                    {
                      pattern: String($event ?? ''),
                    },
                  )
                "
              />
              <UButton
                type="button"
                size="sm"
                color="neutral"
                variant="ghost"
                :icon="
                  redactionStore.isCustomStringRevealed(
                    row.original.array_index,
                  )
                    ? 'i-lucide-eye-off'
                    : 'i-lucide-eye'
                "
                :aria-label="
                  redactionStore.isCustomStringRevealed(
                    row.original.array_index,
                  )
                    ? t('hidePlaintext')
                    : t('showPlaintext')
                "
                @click="
                  redactionStore.toggleCustomStringRevealed(
                    row.original.array_index,
                  )
                "
              />
            </div>
            <RuleWarningBadge
              v-if="hasWarning(row.original.array_index)"
              :row="row"
              :warning="warningForIndex(row.original.array_index)"
              :t="t"
            />
          </div>
        </template>
        <template #matchType-cell="{ row }">
          <UTooltip :text="t(matchTypeTooltipKey(row.original.match_type))">
            <USelect
              :model-value="normalizeMatchType(row.original.match_type)"
              :aria-label="t('matchType')"
              class="w-full"
              :id="`redaction-custom-match-type-${row.original.array_index}`"
              size="sm"
              :items="matchTypeOptions"
              label-key="label"
              value-key="value"
              @update:model-value="
                redactionStore.updateCustomStringRule(
                  row.original.array_index,
                  {
                    match_type: $event,
                  },
                )
              "
            />
          </UTooltip>
        </template>
        <template #scope-cell="{ row }">
          <UTooltip :text="t(scopeTooltipKey(row.original.scope))">
            <USelect
              :model-value="normalizeRuleScope(row.original.scope)"
              :aria-label="t('scopeField')"
              class="w-full"
              :id="`redaction-custom-scope-${row.original.array_index}`"
              size="sm"
              :items="scopeOptions"
              label-key="label"
              value-key="value"
              @update:model-value="
                redactionStore.updateCustomStringRule(
                  row.original.array_index,
                  {
                    scope: $event,
                  },
                )
              "
            />
          </UTooltip>
        </template>
        <template #createdAt-cell="{ row }">
          <span class="text-xs text-dimmed">
            {{ formatDateTime(row.original.created_at) }}
          </span>
        </template>
        <template #updatedAt-cell="{ row }">
          <span class="text-xs text-dimmed">
            {{ formatDateTime(row.original.updated_at) }}
          </span>
        </template>
        <template #actions-cell="{ row }">
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            @click="requestDeleteRule(row.original.array_index)"
          >
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
          </UButton>
        </template>
      </UTable>
      <TablePagination
        :first="customStringFirst"
        :rows="customStringRows"
        :total="customStringTotal"
        :page-size-options="STANDARD_PAGE_SIZE_OPTIONS"
        @change="onCustomStringPage"
      />
    </section>

    <DeleteRuleConfirmDialog
      :open="pendingDeleteRule !== null"
      :rule="pendingDeleteRule"
      :t="t"
      @cancel="cancelDeleteRule"
      @confirm="confirmDeleteRule"
    />
  </div>
</template>
