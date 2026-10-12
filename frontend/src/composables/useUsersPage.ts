import { computed, onMounted } from 'vue'

import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import { useUsersPageActions } from '@/composables/useUsersPageActions'
import { createUsersWorkspaceView } from '@/models/users'
import { useUsersStore } from '@/stores/users'

/**
 * The read-only half of the users workspace.
 *
 * Everything that writes lives in `useUsersPageActions`, which owns the dialogs
 * and the single completion each one is allowed to perform. What is left here
 * is the view: the page's paging state and the workspace projection.
 */
export function useUsersPage() {
  const usersStore = useUsersStore()
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const busy = computed(() => usersStore.loading)

  const actions = useUsersPageActions({
    confirm: (message) => window.confirm(message),
    notifyApiError,
    notifySuccess,
    store: usersStore,
    t,
  })

  const usersWorkspace = computed(() =>
    createUsersWorkspaceView({
      busy: busy.value,
      users: usersStore.pageUsers,
      first: usersStore.first,
      rows: usersStore.rows,
      total: usersStore.total,
    }),
  )

  async function refresh(): Promise<void> {
    try {
      await usersStore.loadPage()
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function onPage(event: TablePageChange): Promise<void> {
    try {
      await usersStore.loadPage(event.first, event.rows)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  onMounted(async () => {
    await refresh()
  })

  return {
    ...actions,
    busy,
    onPage,
    refresh,
    t,
    userPendingActions: computed(() => usersStore.userPendingActions),
    usersWorkspace,
  }
}
