import { onBeforeUnmount, reactive, ref, watch, type Ref } from 'vue'
import type { TokenPlanKeyBadges } from './useTokenPlanBadges'

// #277 P3 crossfade rotation for the endpoint list's inline usage cell.
// Mirrors the dialog's per-key carousel, but operates on the derived badge
// list: 4s autoplay over the `ok` keys, failed keys skipped, hover pauses.
// With fewer than two `ok` keys the timer never starts and the cell stays
// static, so the single-key display is unchanged.
export const TOKEN_PLAN_BADGE_ROTATION_MS = 4000

// Positions of the keys that carry usable numbers. Failed keys are
// excluded so the crossfade never parks on an error card.
export function rotationCandidates(keys: TokenPlanKeyBadges[]): number[] {
  const out: number[] = []
  keys.forEach((key, position) => {
    if (key.ok) out.push(position)
  })
  return out
}

// Next visible position after `current`, wrapping around and skipping
// failed keys. Returns null when fewer than two keys are rotatable, which
// is the signal to keep the cell static.
export function nextRotationIndex(
  keys: TokenPlanKeyBadges[],
  current: number,
): number | null {
  const ok = rotationCandidates(keys)
  if (ok.length < 2) return null
  const position = ok.indexOf(current)
  if (position < 0) return ok[0] ?? null
  return ok[(position + 1) % ok.length] ?? null
}

export function useTokenPlanBadgeRotation(keys: Ref<TokenPlanKeyBadges[]>) {
  const index = ref(0)
  let timer: ReturnType<typeof setInterval> | null = null
  let paused = false

  function stop(): void {
    if (timer !== null) clearInterval(timer)
    timer = null
  }

  function start(): void {
    stop()
    const ok = rotationCandidates(keys.value)
    if (ok.length < 2) {
      index.value = ok[0] ?? 0
      return
    }
    if (!ok.includes(index.value)) index.value = ok[0] ?? 0
    // A data refresh must not resume a rotation the user paused by
    // hovering; only the pointer leaving calls `setPaused(false)`.
    if (paused) return
    timer = setInterval(() => {
      const next = nextRotationIndex(keys.value, index.value)
      if (next !== null) index.value = next
    }, TOKEN_PLAN_BADGE_ROTATION_MS)
  }

  function setPaused(value: boolean): void {
    paused = value
    if (value) stop()
    else start()
  }

  watch(() => keys.value, start, { immediate: true })
  onBeforeUnmount(stop)

  return reactive({ index, setPaused })
}
