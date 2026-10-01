<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRoute } from 'vue-router'
import UsageCategoryPanel from '@/components/usage/UsageCategoryPanel.vue'
import PageIntro from '../components/PageIntro.vue'
import RequestOverviewModeSwitch from '../components/RequestOverviewModeSwitch.vue'
import UsageRangePicker from '../components/usage/UsageRangePicker.vue'
import { useNotifier } from '../composables/useNotifier'
import { useUsagePage } from '../composables/useUsagePage'
import type { RequestOverviewPerspective } from '../request-overview'
import { useRequestRecordsStore } from '../stores/usage'

type UsageSection = 'ai' | 'mcp'

const route = useRoute()
const usageStore = useRequestRecordsStore()
const activeSection = computed<UsageSection>(() =>
  route.path.split('/')[2] === 'mcp' ? 'mcp' : 'ai',
)

if (usageStore.requestCategory !== activeSection.value) {
  usageStore.setRequestCategory(activeSection.value)
}

const {
  UsageClearDialog,
  activeMode,
  applyRange,
  clearDialogVisible,
  clearForm,
  clearConversationOverride,
  detailVisible,
  formatting,
  handleOverviewDrilldown,
  loadDetailRequestFull,
  loadMoreDetailRequestFull,
  onFilter,
  onPage,
  onSort,
  openDetail,
  refresh,
  refreshRecords,
  requestRecordsStore,
  resetSessionAffinity,
  saveConversationOverride,
  session,
  setActiveMode,
  submitClearHistory,
  t,
  usersStore,
} = useUsagePage()

const { notifyApiError } = useNotifier()

async function changeAiPerspective(
  next: RequestOverviewPerspective,
): Promise<void> {
  try {
    await usageStore.setAiPerspective(next)
  } catch (cause) {
    notifyApiError(cause)
  }
}

watch(activeSection, (next) => {
  if (usageStore.requestCategory !== next) {
    usageStore.setRequestCategory(next)
  }
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro>
      <template #actions>
        <UsageRangePicker
          :end="requestRecordsStore.end"
          :start="requestRecordsStore.start"
          :t="t"
          :value="requestRecordsStore.range"
          @apply="applyRange"
        />
        <RequestOverviewModeSwitch
          :active-mode="activeMode"
          @change="setActiveMode"
        />
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :aria-label="t('refresh')"
          @click="refresh"
        >
          <UIcon name="i-lucide-refresh-cw" class="h-4 w-4" />
          <span>{{ t('refresh') }}</span>
        </UButton>
      </template>
    </PageIntro>

    <UsageCategoryPanel
      v-model:filters="requestRecordsStore.filters"
      v-model:detail-visible="detailVisible"
      :active-mode="activeMode"
      :category="activeSection"
      :formatting="formatting"
      :is-admin="session.isAdmin"
      :overview="requestRecordsStore.overview"
      :overview-loading="requestRecordsStore.overviewLoading"
      :perspective="requestRecordsStore.aiPerspective"
      :t="t"
      :workspace="requestRecordsStore.usageWorkspaceView"
      @change-perspective="changeAiPerspective"
      @clear-conversation-override="clearConversationOverride"
      @drilldown="handleOverviewDrilldown"
      @filter="onFilter"
      @load-detail-request-full="loadDetailRequestFull"
      @load-more-detail-request-full="loadMoreDetailRequestFull"
      @open-clear-dialog="clearDialogVisible = true"
      @open-detail="openDetail"
      @page="onPage"
      @reset-session-affinity="resetSessionAffinity"
      @save-conversation-override="saveConversationOverride"
      @search="refreshRecords"
      @sort="onSort"
    />

    <UsageClearDialog
      v-model:visible="clearDialogVisible"
      v-model:form="clearForm"
      :busy="requestRecordsStore.loading"
      :current-user-label="session.displayName || session.loginName"
      :is-admin="session.isAdmin"
      :t="t"
      :users="usersStore.users"
      @submit="submitClearHistory"
    />
  </div>
</template>
