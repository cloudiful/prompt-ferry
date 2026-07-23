<script setup lang="ts">
import type { NavigationMenuItem } from '@nuxt/ui'
import { computed, ref } from 'vue'
import { RouterView, useRoute, useRouter } from 'vue-router'
import { useLocale } from '@/composables/useLocale'
import { visibleNavItems } from '@/nav'
import { useSessionStore } from '@/stores/session'

const collapsed = ref(false)
const route = useRoute()
const router = useRouter()
const session = useSessionStore()
const { t } = useLocale()

const navigationItems = computed<NavigationMenuItem[]>(() =>
  visibleNavItems(session.isAdmin).map((item) => ({
    label: t(item.labelKey as string),
    icon: route.path.startsWith(`/${item.section}`)
      ? item.iconActive
      : item.iconInactive,
    to: `/${item.section}`,
    active: route.path.startsWith(`/${item.section}`),
    children: item.children?.map((child) => ({
      label: t(child.labelKey as string),
      to: child.to,
      active: child.isActive(route),
    })),
  })),
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
      :min-size="14"
      :max-size="22"
      :default-size="17"
    >
      <template #header="{ collapsed: isCollapsed }">
        <div class="flex min-w-0 items-center gap-2 px-1">
          <UIcon name="i-lucide-cable" class="size-5 shrink-0 text-primary" />
          <span v-if="!isCollapsed" class="truncate font-semibold"
            >Prompt Ferry</span
          >
          <UDashboardSidebarCollapse class="ml-auto" />
        </div>
      </template>

      <UNavigationMenu
        :items="navigationItems"
        orientation="vertical"
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
        <UDashboardNavbar title="Prompt Ferry" />
      </template>
      <template #body>
        <RouterView />
      </template>
    </UDashboardPanel>
  </UDashboardGroup>
</template>
