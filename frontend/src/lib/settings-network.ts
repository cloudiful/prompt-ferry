/**
 * The admin Network settings tab: the relay IP whitelist, read by TanStack
 * Query and edited through one native form.
 *
 * The form owns the two textareas. The query owns the stored policy. A save
 * claims the shared `setting:relayIpWhitelist` key, captures the form before
 * the first await, and applies the accepted policy only to the fields the
 * operator has not changed since the submit. A scoped refresh is refused while
 * the form holds an unsaved draft, so a read can never replace it.
 *
 * The form survives an SPA navigation within the session through an in-memory
 * handoff, exactly as the previous module store did; it is never persisted and
 * never crosses a reload or an auth epoch: a retired session's snapshot is
 * discarded at its own epoch boundary, so logout, a new login of the same
 * principal, or a distinct principal all start from the stored policy.
 */

import { useEffect, useRef, useState } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { MeResponse, RelayIpPolicyResponse } from '@/generated/admin-api'
import {
  getRelayIpWhitelistOptions,
  getRelayIpWhitelistQueryKey,
  setRelayIpWhitelistMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { useSessionStore } from './session'
import { useAsyncAction } from './use-async-action'
import {
  createRelayIpWhitelistForm,
  mergeRelayIpWhitelistResponse,
  relayPolicyToWhitelistForm,
  relayWhitelistDirtyFields,
  relayWhitelistFormToRequest,
  RELAY_IP_WHITELIST_KEY,
  relayWhitelistRefreshBlocked,
  type RelayIpWhitelistForm,
} from './settings-network-model'

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

type SettingsNetworkOptions = {
  onSaved?: () => void
}

export function networkIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

/**
 * The epoch of the session that is authenticated right now.
 *
 * A mounted component keeps the epoch it was mounted under as the owner of
 * its own work, but only this live value decides whether that work still
 * belongs to the current session: a component that a logout unmounted never
 * re-renders, so the epoch it captured is stale by the time its continuation
 * runs.
 */
function currentSessionEpoch(): number {
  return useSessionStore.getState().epoch
}

type RetainedNetworkDraft = {
  identityKey: string
  epoch: number
  values: RelayIpWhitelistForm
  baseline: RelayIpWhitelistForm
  loaded: boolean
}

let retainedDraft: RetainedNetworkDraft | null = null

function cloneForm(form: RelayIpWhitelistForm): RelayIpWhitelistForm {
  return { ...form }
}

function readRetainedDraft(
  identityKey: string,
  epoch: number,
): RetainedNetworkDraft | null {
  if (
    !retainedDraft ||
    retainedDraft.identityKey !== identityKey ||
    retainedDraft.epoch !== epoch
  ) {
    // A retired session's snapshot never comes back: same principal or not.
    retainedDraft = null
    return null
  }
  return retainedDraft
}

export function useSettingsNetwork(options: SettingsNetworkOptions = {}) {
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const epoch = useSessionStore((state) => state.epoch)
  const identityKey = networkIdentityKey(me)
  const onSavedRef = useRef(options.onSaved)
  onSavedRef.current = options.onSaved

  const initialRef = useRef<RetainedNetworkDraft | null | undefined>(undefined)
  if (initialRef.current === undefined) {
    initialRef.current = readRetainedDraft(identityKey, currentSessionEpoch())
  }
  const initial = initialRef.current ?? {
    identityKey,
    epoch,
    values: createRelayIpWhitelistForm(),
    baseline: createRelayIpWhitelistForm(),
    loaded: false,
  }

  /**
   * The epoch this form was mounted under.
   *
   * It identifies the owner of every write this form makes. Whether that
   * owner is still in charge is decided by the live session epoch, never by
   * this captured value.
   */
  const mountEpochRef = useRef(epoch)

  const form = useForm({
    defaultValues: cloneForm(initial.values),
    onSubmit: async ({ value }) => {
      const outcome = await saveValues(value)
      if (outcome.executed) onSavedRef.current?.()
    },
  })
  const values = useStore(form.store, (state) => state.values)

  function applyForm(next: RelayIpWhitelistForm): void {
    for (const field of [
      'allowed_cidrs_text',
      'trusted_proxy_cidrs_text',
    ] as const) {
      form.setFieldValue(field, next[field], {
        dontRunListeners: true,
        dontUpdateMeta: true,
        dontValidate: true,
      })
    }
  }
  const [baseline, setBaseline] = useState<RelayIpWhitelistForm>(() =>
    cloneForm(initial.baseline),
  )
  const [saveFailed, setSaveFailed] = useState(false)
  const loadedRef = useRef(initial.loaded)
  const valuesRef = useRef(values)
  const baselineRef = useRef(baseline)
  const queryDataRef = useRef<RelayIpPolicyResponse | undefined>(undefined)
  const retainedRef = useRef<RetainedNetworkDraft>(initial)

  valuesRef.current = values
  baselineRef.current = baseline
  retainedRef.current = {
    identityKey,
    epoch: mountEpochRef.current,
    values: cloneForm(values),
    baseline: cloneForm(baseline),
    loaded: loadedRef.current,
  }

  const query = useQuery({
    ...getRelayIpWhitelistOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })
  const saveMutation = useMutation(setRelayIpWhitelistMutation())

  const loaded = query.data !== undefined
  const loading = query.isFetching && !loaded
  const loadError = query.isError
  const dirty = relayWhitelistDirtyFields(values, baseline).length > 0
  const refreshBlocked =
    relayWhitelistRefreshBlocked({
      pendingActions: asyncAction.pendingActions,
      dirty,
      failed: saveFailed,
      loading: query.isFetching,
      loaded,
    }) || currentSessionEpoch() !== mountEpochRef.current

  useEffect(() => {
    const data = query.data
    if (!data || queryDataRef.current === data) return
    queryDataRef.current = data
    const mapped = relayPolicyToWhitelistForm(data)
    const pristine =
      relayWhitelistDirtyFields(valuesRef.current, baselineRef.current)
        .length === 0
    if (loadedRef.current && !pristine) return
    loadedRef.current = true
    applyForm(mapped)
    setBaseline(mapped)
  }, [form, query.data])

  useEffect(() => {
    const liveEpoch = currentSessionEpoch()
    if (identityKey === '' || liveEpoch !== mountEpochRef.current) {
      // A retired epoch's snapshot is discarded, never rewritten.
      if (retainedDraft && retainedDraft.epoch !== liveEpoch)
        retainedDraft = null
      return
    }
    retainedDraft = {
      identityKey,
      epoch: liveEpoch,
      values: cloneForm(values),
      baseline: cloneForm(baseline),
      loaded: loadedRef.current,
    }
  }, [baseline, epoch, identityKey, values])

  useEffect(() => {
    return () => {
      const latest = retainedRef.current
      const liveEpoch = currentSessionEpoch()
      retainedDraft =
        latest.identityKey === '' || latest.epoch !== liveEpoch
          ? null
          : {
              identityKey: latest.identityKey,
              epoch: liveEpoch,
              values: cloneForm(latest.values),
              baseline: cloneForm(latest.baseline),
              loaded: latest.loaded,
            }
    }
  }, [])

  async function refresh(): Promise<void> {
    if (identityKey === '' || currentSessionEpoch() !== mountEpochRef.current)
      return
    await queryClient.fetchQuery({
      ...getRelayIpWhitelistOptions(),
      staleTime: 0,
    })
  }

  async function saveValues(
    value: RelayIpWhitelistForm,
  ): Promise<AsyncActionOutcome<void>> {
    if (
      identityKey === '' ||
      currentSessionEpoch() !== mountEpochRef.current ||
      !loaded
    ) {
      return { executed: false }
    }
    const submitted = capturePayload(value)
    const body = relayWhitelistFormToRequest(submitted)
    try {
      const outcome = await runAsyncAction(
        RELAY_IP_WHITELIST_KEY,
        async () => {
          const response = await saveMutation.mutateAsync({ body })
          if (currentSessionEpoch() !== mountEpochRef.current) return
          const latest = valuesRef.current
          applyForm(mergeRelayIpWhitelistResponse(latest, submitted, response))
          setBaseline(relayPolicyToWhitelistForm(response))
          loadedRef.current = true
          queryClient.setQueryData(getRelayIpWhitelistQueryKey(), response)
        },
        'settings',
      )
      if (currentSessionEpoch() !== mountEpochRef.current)
        return { executed: false }
      if (outcome.executed) setSaveFailed(false)
      return outcome
    } catch (cause) {
      // A late callback from a retired session neither marks the form failed
      // nor surfaces its error to the account that is logged in now.
      if (currentSessionEpoch() !== mountEpochRef.current)
        return { executed: false }
      setSaveFailed(true)
      throw cause
    }
  }

  return {
    form,
    values,
    loaded,
    loading,
    loadError,
    dirty,
    saveFailed,
    refresh,
    refreshBlocked,
    pending: asyncAction.isPending(RELAY_IP_WHITELIST_KEY),
    pendingOrigin: asyncAction.pendingOrigin(RELAY_IP_WHITELIST_KEY),
  }
}
