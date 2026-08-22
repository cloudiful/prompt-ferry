<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, onMounted, ref } from 'vue'
import QuotaGroupDialog from '@/components/mcp/QuotaGroupDialog.vue'
import {
  createQuotaGroup,
  deleteQuotaGroup,
  listQuotaGroups,
  quotaGroupUsage,
  updateQuotaGroup,
} from '@/generated/admin-api'
import type {
  McpQuotaGroup,
  QuotaGroupRequest,
  QuotaGroupUsageResponse,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { useNotifier } from '@/composables/useNotifier'
import { useLocale } from '@/composables/useLocale'

const { t } = useLocale()
const { notifyApiError, notifySuccess } = useNotifier()

const groups = ref<McpQuotaGroup[]>([])
const usage = ref<Record<string, QuotaGroupUsageResponse>>({})
const loading = ref(false)
const saving = ref(false)
const dialogVisible = ref(false)
const dialogGroup = ref<McpQuotaGroup | null>(null)

type GroupRow = {
  group: McpQuotaGroup
  usage: QuotaGroupUsageResponse | undefined
}

const rows = computed<GroupRow[]>(() =>
  groups.value.map((group) => ({
    group,
    usage: usage.value[group.group_id],
  })),
)

const columns = computed<TableColumn<GroupRow>[]>(() => [
  { accessorKey: 'name', header: t('quotaGroupName') },
  { id: 'unit', header: t('quotaUnit') },
  { id: 'limits', header: t('monthlyCallLimit') },
  { id: 'usage', header: t('quotaUsage') },
  { id: 'actions' },
])

function unitLabel(group: McpQuotaGroup): string {
  return group.unit === 'credits'
    ? t('quotaUnitCredits')
    : t('quotaUnitRequests')
}

function limitLabel(group: McpQuotaGroup): string {
  const limit = group.monthly_limit
  return limit == null ? t('quotaUnlimited') : String(limit)
}

function usageNumbers(group: McpQuotaGroup): {
  used: number
  reserved: number
  limit: number
} {
  const account = usage.value[group.group_id]?.month
  const limit = group.monthly_limit ?? 0
  return {
    used: account?.used_units ?? 0,
    reserved: account?.reserved_units ?? 0,
    limit,
  }
}

function usagePercent(group: McpQuotaGroup): number {
  const { used, reserved, limit } = usageNumbers(group)
  if (limit <= 0) return 0
  return Math.min(100, Math.round(((used + reserved) / limit) * 100))
}

function usageBadge(group: McpQuotaGroup): { color: string; label: string } {
  const { used, reserved, limit } = usageNumbers(group)
  if (limit <= 0) {
    return { color: 'neutral', label: t('quotaNotConfigured') }
  }
  const remaining = Math.max(0, limit - used - reserved)
  if (remaining <= 0) {
    return { color: 'error', label: t('quotaExhausted') }
  }
  return {
    color: remaining / limit <= 0.2 ? 'warning' : 'success',
    label: `${t('quotaRemaining')} ${remaining}`,
  }
}

const dialogHeader = computed(() =>
  dialogGroup.value ? t('editQuotaGroup') : t('newQuotaGroup'),
)

async function refresh(): Promise<void> {
  loading.value = true
  try {
    groups.value = expectData(await listQuotaGroups<true>(withData({})))
    await Promise.all(
      groups.value.map(async (group) => {
        try {
          usage.value[group.group_id] = expectData(
            await quotaGroupUsage<true>(
              withData({ path: { group_id: group.group_id } }),
            ),
          )
        } catch {
          // Usage detail is best-effort; the group row still renders.
        }
      }),
    )
  } catch (cause) {
    notifyApiError(cause)
  } finally {
    loading.value = false
  }
}

function openCreate(): void {
  dialogGroup.value = null
  dialogVisible.value = true
}

function openEdit(group: McpQuotaGroup): void {
  dialogGroup.value = group
  dialogVisible.value = true
}

async function save(request: QuotaGroupRequest): Promise<void> {
  saving.value = true
  try {
    if (dialogGroup.value) {
      await updateQuotaGroup<true>({
        body: request,
        path: { group_id: dialogGroup.value.group_id },
      })
    } else {
      await createQuotaGroup<true>({ body: request })
    }
    dialogVisible.value = false
    notifySuccess(t('saved'))
    await refresh()
  } catch (cause) {
    notifyApiError(cause)
  } finally {
    saving.value = false
  }
}

async function remove(group: McpQuotaGroup): Promise<void> {
  if (!window.confirm(t('quotaGroupDeleteConfirm'))) return
  try {
    await deleteQuotaGroup<true>({
      path: { group_id: group.group_id },
    })
    notifySuccess(t('delete'))
    await refresh()
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(refresh)

defineExpose({ refresh })
</script>

<template>
  <section class="grid min-w-0 gap-3">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="grid gap-1">
        <div class="flex items-center gap-1">
          <div class="text-sm font-semibold">{{ t('quotaGroups') }}</div>
          <UTooltip :text="t('quotaGroupsHint')">
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-info"
              :aria-label="t('quotaGroupsHint')"
            />
          </UTooltip>
        </div>
      </div>
      <UButton size="sm" @click="openCreate">
        <UIcon name="i-lucide-plus" class="h-4 w-4" />
        <span>{{ t('newQuotaGroup') }}</span>
      </UButton>
    </div>
    <UTable :data="rows" :columns="columns" :loading="loading" class="min-w-0">
      <template #empty>
        <div class="px-4 py-6 text-sm text-dimmed">
          {{ t('quotaNotConfigured') }}
        </div>
      </template>
      <template #name-cell="{ row }">
        <div class="min-w-0">
          <div class="truncate font-semibold text-highlighted">
            {{ row.original.group.name }}
          </div>
          <div
            v-if="row.original.group.provider_kind"
            class="truncate text-xs text-muted"
          >
            {{ row.original.group.provider_kind }}
          </div>
        </div>
      </template>
      <template #unit-cell="{ row }">
        <UBadge :label="unitLabel(row.original.group)" color="info" />
      </template>
      <template #limits-cell="{ row }">
        <div class="text-xs">
          <div>
            {{ t('monthlyCallLimit') }}: {{ limitLabel(row.original.group) }}
          </div>
          <div class="text-muted">
            {{ t('dailyCallLimit') }}:
            {{
              row.original.group.daily_limit == null
                ? t('quotaUnlimited')
                : row.original.group.daily_limit
            }}
            ·
            {{ t('defaultCost') }}: {{ row.original.group.default_cost }}
          </div>
        </div>
      </template>
      <template #usage-cell="{ row }">
        <div
          v-if="usageNumbers(row.original.group).limit > 0"
          class="grid w-40 gap-1"
        >
          <div class="flex items-center justify-between text-xs">
            <span class="text-muted">
              {{ t('quotaUsed') }}
              {{ usageNumbers(row.original.group).used }}
              <template v-if="usageNumbers(row.original.group).reserved > 0">
                +
                {{ t('quotaReserved') }}
                {{ usageNumbers(row.original.group).reserved }}
              </template>
            </span>
            <UBadge
              :color="usageBadge(row.original.group).color"
              :label="usageBadge(row.original.group).label"
              size="sm"
            />
          </div>
          <UProgress
            :model-value="usagePercent(row.original.group)"
            :color="
              usageBadge(row.original.group).color === 'error'
                ? 'error'
                : usageBadge(row.original.group).color === 'warning'
                  ? 'warning'
                  : 'primary'
            "
          />
        </div>
        <span v-else class="text-xs text-dimmed">
          {{ t('quotaNotConfigured') }}
        </span>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end gap-1">
          <UButton
            size="sm"
            color="neutral"
            variant="outline"
            :icon="'i-lucide-pencil'"
            :aria-label="t('editQuotaGroup')"
            @click="openEdit(row.original.group)"
          />
          <UButton
            size="sm"
            color="error"
            variant="outline"
            :icon="'i-lucide-trash'"
            :aria-label="t('deleteQuotaGroup')"
            @click="remove(row.original.group)"
          />
        </div>
      </template>
    </UTable>
    <QuotaGroupDialog
      v-model:visible="dialogVisible"
      :busy="saving"
      :group="dialogGroup"
      :header="dialogHeader"
      :t="t"
      @save="save"
    />
  </section>
</template>
