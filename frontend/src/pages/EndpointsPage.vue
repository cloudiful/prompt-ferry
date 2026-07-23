<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import PageIntro from '../components/PageIntro.vue'
import EndpointDialog from '../components/endpoints/EndpointDialog.vue'
import EndpointsEndpointsTab from '../components/endpoints/EndpointsEndpointsTab.vue'
import EndpointsRoutesTab from '../components/endpoints/EndpointsRoutesTab.vue'
import ModelRouteDialog from '../components/endpoints/ModelRouteDialog.vue'
import { useEndpointsPage } from '../composables/useEndpointsPage'

type EndpointsSection = 'upstreams' | 'routes'

const {
  busy,
  deleteEndpoint,
  deleteModelRoute,
  editEndpoint,
  editModelRoute,
  endpointDialogHeader,
  endpointDialogVisible,
  endpointForm,
  endpointsStore,
  modelRouteDialogHeader,
  modelRouteDialogVisible,
  modelRouteForm,
  onEndpointPage,
  onModelRoutePage,
  openEndpointDialog,
  openModelRouteDialog,
  refresh,
  saveEndpoint,
  saveModelRoute,
  t,
  testEndpoint,
  testModelRoute,
  toggleEndpointEnabled,
  toggleModelRouteEnabled,
  usersStore,
} = useEndpointsPage()

const route = useRoute()

const activeSection = computed<EndpointsSection>(() => {
  const tab = route.path.split('/')[2] ?? 'upstreams'
  if (tab === 'routes') return tab
  return 'upstreams'
})

const pageTitle = computed(() => {
  if (activeSection.value === 'routes') return t('modelRoute')
  return t('endpoint')
})

const showNewEndpointButton = computed(
  () => activeSection.value === 'upstreams',
)
const showNewModelRouteButton = computed(() => activeSection.value === 'routes')
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro :title="pageTitle">
      <template #actions>
        <UButton
          v-if="showNewEndpointButton"
          size="sm"
          :aria-label="t('newEndpoint')"
          @click="openEndpointDialog"
        >
          <span aria-hidden="true" class="md:hidden">新增上游</span>
          <span aria-hidden="true" class="hidden md:inline">{{
            t('newEndpoint')
          }}</span>
        </UButton>
        <UButton
          v-if="showNewModelRouteButton"
          size="sm"
          :aria-label="t('newModelRoute')"
          @click="openModelRouteDialog"
        >
          <span aria-hidden="true" class="md:hidden">新增路由</span>
          <span aria-hidden="true" class="hidden md:inline">{{
            t('newModelRoute')
          }}</span>
        </UButton>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="busy"
          :aria-label="t('refresh')"
          @click="refresh"
        >
          <span>{{ t('refresh') }}</span>
        </UButton>
      </template>
    </PageIntro>

    <EndpointsEndpointsTab
      v-if="activeSection === 'upstreams'"
      :workspace="endpointsStore.selectedWorkspaceView"
      :t="t"
      @delete-endpoint="deleteEndpoint"
      @edit-endpoint="editEndpoint"
      @endpoint-page="onEndpointPage"
      @test-endpoint="testEndpoint"
      @toggle-endpoint-enabled="toggleEndpointEnabled"
    />

    <EndpointsRoutesTab
      v-else-if="activeSection === 'routes'"
      :workspace="endpointsStore.selectedWorkspaceView"
      :t="t"
      @delete-model-route="deleteModelRoute"
      @edit-model-route="editModelRoute"
      @model-route-page="onModelRoutePage"
      @test-model-route="testModelRoute"
      @toggle-model-route-enabled="toggleModelRouteEnabled"
    />

    <EndpointDialog
      v-model:visible="endpointDialogVisible"
      v-model:form="endpointForm"
      :busy="busy"
      :header="endpointDialogHeader"
      :t="t"
      :users="usersStore.users"
      @save="saveEndpoint"
    />

    <ModelRouteDialog
      v-model:visible="modelRouteDialogVisible"
      v-model:form="modelRouteForm"
      :busy="busy"
      :endpoint-options="endpointsStore.endpointOptions"
      :endpoints="endpointsStore.endpoints"
      :header="modelRouteDialogHeader"
      :t="t"
      :users="usersStore.users"
      @save="saveModelRoute"
    />
  </div>
</template>
