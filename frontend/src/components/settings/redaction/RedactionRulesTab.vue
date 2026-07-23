<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type {
  RedactionConfigSchema,
  RedactionCustomStringRuleRowSchema,
} from '@/generated/admin-api'
import TablePagination from '@/components/shared/TablePagination.vue'
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
  customStringUpdatedAt,
  customStrings,
} = storeToRefs(redactionStore)

const customStringSearchModel = computed({
  get: () => customStringSearch.value,
  set: (value: string) => {
    void redactionStore.setCustomStringSearch(value)
  },
})

function onCustomStringPage(event: TablePageChange): void {
  void redactionStore.setCustomStringPage(event.first, event.rows)
}

function formatUpdatedAt(value: string | null): string {
  if (!value) return '-'
  const date = new Date(value)
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString()
}
</script>

<template>
  <div class="grid gap-3">
    <section class="grid gap-3">
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
      <UTable :data="customStrings" :columns="columns" class="min-w-0">
        <template #empty>{{ t('noCustomStrings') }}</template>
        <template #pattern-cell="{ row }">
          <UInput
            :model-value="row.original.pattern"
            type="password"
            class="w-full"
            name="redaction-custom-pattern"
            size="sm"
            @update:model-value="
              redactionStore.updateCustomStringRule(row.original.array_index, {
                pattern: String($event ?? ''),
              })
            "
          />
        </template>
        <template #matchType-cell="{ row }">
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
              redactionStore.updateCustomStringRule(row.original.array_index, {
                match_type: $event,
              })
            "
          />
        </template>
        <template #scope-cell="{ row }">
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
              redactionStore.updateCustomStringRule(row.original.array_index, {
                scope: $event,
              })
            "
          />
        </template>
        <template #updatedAt-cell>
          <span class="text-xs text-dimmed">
            {{ formatUpdatedAt(customStringUpdatedAt) }}
          </span>
        </template>
        <template #actions-cell="{ row }">
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            @click="
              redactionStore.removeCustomStringRule(row.original.array_index)
            "
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
  </div>
</template>
