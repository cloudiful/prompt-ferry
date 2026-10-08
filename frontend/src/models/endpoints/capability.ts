import type {
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
} from '@/generated/admin-api'

export type WorkerLinkState = 'connected' | 'disconnected'

export function workerLinkState(
  status: RelayStatusResponse | null | undefined,
): WorkerLinkState {
  return status?.worker.connected === true ? 'connected' : 'disconnected'
}

export function isWorkerConnected(
  status: RelayStatusResponse | null | undefined,
): boolean {
  return workerLinkState(status) === 'connected'
}

export function isRelayReady(
  status: RelayStatusResponse | null | undefined,
): boolean {
  return status?.relay_ready === true
}

export function isHostRestartRequired(
  host: RelayHostResponse | null | undefined,
  status: RelayStatusResponse | null | undefined,
  settings: RelaySettingsResponse | null | undefined,
): boolean {
  return (
    host?.restart_required === true ||
    status?.restart_required === true ||
    settings?.restart_required === true
  )
}

export function canManageWorker(
  status: RelayStatusResponse | null | undefined,
): boolean {
  return isWorkerConnected(status)
}
