<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'
import { computed, ref, watch } from 'vue'
import { RouterView, useRoute, useRouter } from 'vue-router'
import { useLocale } from '@/composables/useLocale'
import { visibleNavItems } from '@/nav'
import { useSessionStore } from '@/stores/session'

const collapsed = ref(false)
const route = useRoute()
const router = useRouter()
const session = useSessionStore()
const { t } = useLocale()
const openNavigationSections = ref<string[]>([])

const navigationItems = computed<NavigationMenuItem[]>(() =>
  visibleNavItems(session.isAdmin).map((item) => ({
    label: t(item.labelKey as string),
    value: item.section,
    icon: route.path.startsWith(`/${item.section}`)
      ? item.iconActive
      : item.iconInactive,
    to: `/${item.section}`,
    active: route.path.startsWith(`/${item.section}`),
    type: item.children?.length ? 'trigger' : 'link',
    children: item.children?.map((child) => ({
      label: t(child.labelKey as string),
      to: child.to,
      active: child.isActive(route),
    })),
  })),
)

const activeExpandableSection = computed(() => {
  const item = visibleNavItems(session.isAdmin).find(
    (navItem) =>
      navItem.children?.length && route.path.startsWith(`/${navItem.section}`),
  )
  return item?.section
})

watch(
  activeExpandableSection,
  (section) => {
    if (section && !openNavigationSections.value.includes(section)) {
      openNavigationSections.value = [...openNavigationSections.value, section]
    }
  },
  { immediate: true },
)

async function logout(): Promise<void> {
  await session.logout()
  await router.replace('/login')
}
</script>

<template>
  <UDashboardGroup storage-key="prompt-ferry:dashboard" class="min-h-screen">
    <UDashboardSidebar
      v-model:collapsed="collapsed"
      collapsible
      resizable
      :min-size="12"
      :max-size="16"
      :default-size="13"
    >
      <template #header>
        <div class="flex min-w-0 items-center justify-end px-1">
          <UDashboardSidebarCollapse />
        </div>
      </template>

      <UNavigationMenu
        v-model="openNavigationSections"
        :items="navigationItems"
        orientation="vertical"
        type="multiple"
        :collapsed="collapsed"
        tooltip
        popover
      />

      <template #footer="{ collapsed: isCollapsed }">
        <div class="grid gap-2">
          <span v-if="!isCollapsed" class="truncate px-2 text-xs text-muted">
            {{ session.loginName }}
          </span>
          <UButton
            color="neutral"
            variant="ghost"
            icon="i-lucide-log-out"
            :label="isCollapsed ? undefined : t('logout')"
            :aria-label="t('logout')"
            :block="!isCollapsed"
            @click="logout"
          />
        </div>
      </template>
    </UDashboardSidebar>

    <UDashboardPanel>
      <template #header>
        <UDashboardNavbar :ui="{ right: 'min-w-0 flex-1 justify-end' }">
          <template #right>
            <div
              id="dashboard-navbar-actions"
              class="flex min-w-0 max-w-full items-center justify-end gap-1.5 overflow-x-auto overscroll-x-contain"
            />
          </template>
        </UDashboardNavbar>
      </template>
      <template #body>
        <RouterView />
      </template>
    </UDashboardPanel>
  </UDashboardGroup>
</template>
