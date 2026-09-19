import { ref } from 'vue'

export type RowRevealState = {
  clear: () => void
  hide: (index: number) => void
  isRevealed: (index: number) => boolean
  remove: (index: number) => void
  reveal: (index: number) => void
  toggle: (index: number) => void
}

export function createRowRevealState(): RowRevealState {
  const revealed = ref<ReadonlySet<number>>(new Set<number>())

  function reveal(index: number): void {
    if (revealed.value.has(index)) return
    const next = new Set(revealed.value)
    next.add(index)
    revealed.value = next
  }

  function hide(index: number): void {
    if (!revealed.value.has(index)) return
    const next = new Set(revealed.value)
    next.delete(index)
    revealed.value = next
  }

  function toggle(index: number): void {
    if (revealed.value.has(index)) hide(index)
    else reveal(index)
  }

  function remove(index: number): void {
    if (revealed.value.size === 0) return
    const next = new Set<number>()
    for (const current of revealed.value) {
      if (current === index) continue
      next.add(current > index ? current - 1 : current)
    }
    revealed.value = next
  }

  function clear(): void {
    if (revealed.value.size === 0) return
    revealed.value = new Set<number>()
  }

  return {
    clear,
    hide,
    isRevealed: (index: number) => revealed.value.has(index),
    remove,
    reveal,
    toggle,
  }
}
