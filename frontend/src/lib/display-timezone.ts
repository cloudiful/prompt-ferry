import { useSyncExternalStore } from 'react'
import {
  DISPLAY_TIME_ZONE_STORAGE_KEY,
  readStorage,
  writeStorage,
} from '@/storage'
import {
  effectiveTimeZoneFor,
  resolveDisplayTimeZone,
} from '@/format/timezone-core'

export type DisplayTimeZoneSnapshot = {
  selected: string
  effective: string
}

type Listener = () => void

let selected = resolveDisplayTimeZone(
  readStorage(DISPLAY_TIME_ZONE_STORAGE_KEY),
)
let snapshot = createSnapshot(selected)
const listeners = new Set<Listener>()
let listeningForStorage = false

function createSnapshot(value: string): DisplayTimeZoneSnapshot {
  return Object.freeze({
    selected: value,
    effective: effectiveTimeZoneFor(value),
  })
}

function publish(next: string | null): void {
  const normalized = resolveDisplayTimeZone(next)
  if (normalized === selected) return
  selected = normalized
  snapshot = createSnapshot(normalized)
  for (const listener of listeners) listener()
}

function handleStorage(event: StorageEvent): void {
  if (event.key !== DISPLAY_TIME_ZONE_STORAGE_KEY && event.key !== null) return
  publish(event.newValue)
}

function subscribe(listener: Listener): () => void {
  listeners.add(listener)
  if (!listeningForStorage && typeof window !== 'undefined') {
    window.addEventListener('storage', handleStorage)
    listeningForStorage = true
  }
  return () => {
    listeners.delete(listener)
    if (listeners.size === 0 && listeningForStorage) {
      window.removeEventListener('storage', handleStorage)
      listeningForStorage = false
    }
  }
}

export function getDisplayTimeZoneSnapshot(): DisplayTimeZoneSnapshot {
  return snapshot
}

export function getDisplayTimeZone(): string {
  return selected
}

export function getEffectiveDisplayTimeZone(): string {
  return snapshot.effective
}

export function setDisplayTimeZone(next: string): void {
  const normalized = resolveDisplayTimeZone(next)
  if (normalized === selected) return
  selected = normalized
  snapshot = createSnapshot(normalized)
  writeStorage(DISPLAY_TIME_ZONE_STORAGE_KEY, normalized)
  for (const listener of listeners) listener()
}

export function useDisplayTimeZone(): DisplayTimeZoneSnapshot {
  return useSyncExternalStore(
    subscribe,
    getDisplayTimeZoneSnapshot,
    getDisplayTimeZoneSnapshot,
  )
}
