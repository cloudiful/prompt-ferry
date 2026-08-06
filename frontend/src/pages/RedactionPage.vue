<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import PageIntro from '../components/PageIntro.vue'
import RedactionPreviewTab from '../components/settings/redaction/RedactionPreviewTab.vue'
import RedactionRulesTab from '../components/settings/redaction/RedactionRulesTab.vue'
import { useLocale } from '../composables/useLocale'
import { useNotifier } from '../composables/useNotifier'
import { createRedactionWorkspaceView } from '../models/redaction'
import { useRedactionStore } from '../stores/redaction'
import { useSessionStore } from '../stores/session'
import { useUsersStore } from '../stores/users'

const { t } = useLocale()
const { notifyApiError, notifySuccess } = useNotifier()
const redactionStore = useRedactionStore()
const route = useRoute()
const sessionStore = useSessionStore()
const usersStore = useUsersStore()
const activeSection = computed(() =>
  route.path.split('/')[2] === 'preview' ? 'preview' : 'rules',
)
const redactionWorkspace = computed(() =>
  createRedactionWorkspaceView({
    config: redactionStore.config,
    previewResult: redactionStore.previewResult,
    t,
  }),
)

const scopeOptions = computed(() =>
  sessionStore.isAdmin
    ? [
        { label: t('redactionPublicRules'), value: 'global' },
        { label: t('redactionPrivateRules'), value: 'user' },
      ]
    : [{ label: t('redactionMyPrivateRules'), value: 'user' }],
)

const userOptions = computed(() =>
  usersStore.users.map((user) => ({
    label: `${user.display_name || user.login_name} · ${user.login_name}`,
    value: user.user_id,
  })),
)

function defaultTargetUserId(): number | null {
  return usersStore.users[0]?.user_id ?? sessionStore.me?.user_id ?? null
}

async function changeScope(scope: string): Promise<void> {
  if (scope !== 'global' && scope !== 'user') return
  redactionStore.setTarget(
    scope,
    scope === 'user'
      ? (redactionStore.targetUserId ?? defaultTargetUserId())
      : null,
  )
  await refresh()
}

async function changeTargetUser(userId: number): Promise<void> {
  redactionStore.setTarget('user', userId)
  await refresh()
}

async function refresh(): Promise<void> {
  try {
    await redactionStore.refresh()
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function save(): Promise<void> {
  try {
    await redactionStore.save()
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function runPreview(): Promise<void> {
  try {
    await redactionStore.runPreview()
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(async () => {
  if (sessionStore.isAdmin) {
    await usersStore.loadUsers()
    redactionStore.setTarget('global', null)
  } else {
    redactionStore.setTarget('user', sessionStore.me?.user_id ?? null)
  }
  await refresh()
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro>
      <template #actions>
        <div class="flex min-w-0 items-center gap-1.5 whitespace-nowrap">
          <span class="text-[0.72rem] text-muted">{{
            t('redactionRuleScope')
          }}</span>
          <USelect
            v-model="redactionStore.scope"
            size="sm"
            :items="scopeOptions"
            label-key="label"
            value-key="value"
            class="w-28"
            :disabled="redactionStore.loading"
            :aria-label="t('redactionRuleScope')"
            @update:model-value="changeScope"
          />
        </div>
        <div
          v-if="sessionStore.isAdmin && redactionStore.scope === 'user'"
          class="flex min-w-0 items-center gap-1.5 whitespace-nowrap"
        >
          <span class="text-[0.72rem] text-muted">{{
            t('redactionTargetUser')
          }}</span>
          <USelect
            :model-value="redactionStore.targetUserId ?? undefined"
            size="sm"
            :items="userOptions"
            label-key="label"
            value-key="value"
            class="w-44"
            :disabled="redactionStore.loading"
            :aria-label="t('redactionTargetUser')"
            @update:model-value="changeTargetUser($event ?? null)"
          />
        </div>
        <UButton size="sm" :loading="redactionStore.loading" @click="save">{{
          t('save')
        }}</UButton>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="redactionStore.loading"
          @click="refresh"
          >{{ t('refresh') }}</UButton
        >
        <label class="inline-flex min-h-8 items-center whitespace-nowrap">
          <USwitch
            v-model="redactionStore.config.enabled"
            id="redaction-enabled-top"
            :aria-label="t('redaction')"
          />
        </label>
      </template>
    </PageIntro>

    <RedactionRulesTab
      v-if="activeSection === 'rules'"
      v-model:config="redactionStore.config"
      :t="t"
      :workspace="redactionWorkspace"
    />

    <RedactionPreviewTab
      v-else
      v-model:preview-text="redactionStore.previewText"
      v-model:preview-input-kind="redactionStore.previewInputKind"
      v-model:preview-result="redactionStore.previewResult"
      :busy="redactionStore.loading"
      :t="t"
      :workspace="redactionWorkspace"
      @run-preview="runPreview"
    />
  </div>
</template>
