<script setup lang="ts">
import type { HostRole } from '@/generated/admin-api'
import { useLocale } from '@/composables/useLocale'

defineProps<{
  currentRole: HostRole | null
  pendingRole?: HostRole | null
  busy: boolean
}>()

defineEmits<{ select: [role: HostRole] }>()

const { t } = useLocale()

const roles: Array<{
  role: HostRole
  titleKey: 'relayRoleIntegrated' | 'relayRoleWorker' | 'relayRoleRelay'
  descKey:
    'relayRoleIntegratedDesc' | 'relayRoleWorkerDesc' | 'relayRoleRelayDesc'
}> = [
  {
    role: 'integrated',
    titleKey: 'relayRoleIntegrated',
    descKey: 'relayRoleIntegratedDesc',
  },
  {
    role: 'worker',
    titleKey: 'relayRoleWorker',
    descKey: 'relayRoleWorkerDesc',
  },
  {
    role: 'relay',
    titleKey: 'relayRoleRelay',
    descKey: 'relayRoleRelayDesc',
  },
]
</script>

<template>
  <div class="grid gap-3 md:grid-cols-3">
    <UCard
      v-for="item in roles"
      :key="item.role"
      :class="currentRole === item.role ? 'ring-2 ring-primary' : ''"
    >
      <template #header>
        <div class="flex items-center justify-between gap-2">
          <span class="text-sm font-semibold text-highlighted">{{
            t(item.titleKey)
          }}</span>
          <UBadge
            v-if="currentRole === item.role"
            color="primary"
            :label="t('relayRoleCurrent')"
          />
          <UBadge
            v-else-if="pendingRole === item.role"
            color="warning"
            variant="subtle"
            :label="t('relayRolePending')"
          />
        </div>
      </template>
      <p class="min-h-10 text-sm text-muted">{{ t(item.descKey) }}</p>
      <template #footer>
        <UButton
          size="sm"
          :color="currentRole === item.role ? 'neutral' : 'primary'"
          :variant="currentRole === item.role ? 'outline' : 'solid'"
          :loading="busy"
          :disabled="busy || currentRole === item.role"
          @click="$emit('select', item.role)"
        >
          {{
            currentRole === item.role
              ? t('relayRoleSelected')
              : t('relayUseRole')
          }}
        </UButton>
      </template>
    </UCard>
  </div>
</template>
