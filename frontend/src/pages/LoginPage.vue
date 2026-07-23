<script setup lang="ts">
import type { FormError, FormSubmitEvent } from '@nuxt/ui'
import { computed, reactive, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import LocaleToggle from '@/components/shared/LocaleToggle.vue'
import { useLocale } from '@/composables/useLocale'
import { defaultNavSection } from '@/nav'
import { LOGIN_NAME_STORAGE_KEY, readStorage, writeStorage } from '@/storage'
import { useSessionStore } from '@/stores/session'

type LoginForm = {
  login_name: string
  password: string
}

const session = useSessionStore()
const route = useRoute()
const router = useRouter()
const { t } = useLocale()
const busy = computed(() => session.busy)
const showPassword = ref(false)
const state = reactive<LoginForm>({
  login_name: loadInitialLoginName(),
  password: '',
})

function loadInitialLoginName(): string {
  try {
    return readStorage(LOGIN_NAME_STORAGE_KEY) ?? 'admin'
  } catch {
    return 'admin'
  }
}

function rememberLoginName(loginName: string): void {
  try {
    writeStorage(LOGIN_NAME_STORAGE_KEY, loginName)
  } catch {
    // Storage is optional; authentication must remain available.
  }
}

function validate(value: Partial<LoginForm>): FormError[] {
  const errors: FormError[] = []
  if (!value.login_name?.trim()) {
    errors.push({ name: 'login_name', message: t('loginNameRequired') })
  }
  if (!value.password) {
    errors.push({ name: 'password', message: t('passwordRequired') })
  }
  return errors
}

async function submit(event: FormSubmitEvent<LoginForm>): Promise<void> {
  const loginName = event.data.login_name.trim()
  try {
    rememberLoginName(loginName)
    await session.login({
      login_name: loginName,
      password: event.data.password,
    })
    const redirect =
      typeof route.query.redirect === 'string'
        ? route.query.redirect
        : `/${defaultNavSection(session.isAdmin)}`
    await router.replace(redirect)
  } catch {
    // Session store exposes the localized API error below.
  }
}
</script>

<template>
  <main class="grid min-h-screen place-items-center bg-default p-5 sm:p-8">
    <UCard class="w-full max-w-md">
      <template #header>
        <h1 class="text-lg font-semibold text-highlighted">
          {{ t('workspaceTitle') }}
        </h1>
      </template>

      <UForm
        :state="state"
        :validate="validate"
        class="grid gap-4"
        @submit="submit"
      >
        <UFormField :label="t('loginName')" name="login_name" required>
          <UInput
            v-model="state.login_name"
            autocomplete="username"
            :placeholder="t('loginName')"
            autofocus
            class="w-full"
          />
        </UFormField>

        <UFormField :label="t('password')" name="password" required>
          <UInput
            v-model="state.password"
            :type="showPassword ? 'text' : 'password'"
            autocomplete="current-password"
            :placeholder="t('password')"
            class="w-full"
          >
            <template #trailing>
              <UButton
                color="neutral"
                variant="ghost"
                size="xs"
                :icon="showPassword ? 'i-lucide-eye-off' : 'i-lucide-eye'"
                :aria-label="showPassword ? t('hideSecret') : t('showSecret')"
                @click="
                  () => {
                    showPassword = !showPassword
                  }
                "
              />
            </template>
          </UInput>
        </UFormField>

        <UAlert
          v-if="session.error"
          color="error"
          variant="subtle"
          icon="i-lucide-circle-alert"
          :description="session.error"
        />

        <UButton
          type="submit"
          icon="i-lucide-log-in"
          :label="t('login')"
          :loading="busy"
          block
        />
      </UForm>

      <template #footer>
        <div class="flex justify-center">
          <LocaleToggle />
        </div>
      </template>
    </UCard>
  </main>
</template>
