import { ref, type Ref } from 'vue'
import type { ModelRouteForm } from '@/models'

export function useModelRouteTargetDrag(form: Ref<ModelRouteForm>) {
  const dragFromIndex = ref<number | null>(null)
  const dragOverIndex = ref<number | null>(null)

  function moveTargetTo(from: number, to: number): void {
    const targets = form.value?.targets
    if (!Array.isArray(targets)) return
    if (from < 0 || from >= targets.length) return
    if (to < 0 || to >= targets.length) return
    if (from === to) return
    const [target] = targets.splice(from, 1)
    if (target) targets.splice(to, 0, target)
  }

  function isGripTarget(target: EventTarget | null): boolean {
    const element = target as HTMLElement | null
    return element?.closest?.('[data-target-drag-handle]') != null
  }

  function onRowDragStart(event: DragEvent, index: number): void {
    if (!isGripTarget(event.target)) {
      event.preventDefault()
      return
    }
    dragFromIndex.value = index
    if (event.dataTransfer) {
      event.dataTransfer.setData('text/plain', String(index))
      event.dataTransfer.effectAllowed = 'move'
    }
  }

  function onRowDragOver(event: DragEvent, index: number): void {
    event.preventDefault()
    dragOverIndex.value = index
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  }

  function onDrop(event: DragEvent, index: number): void {
    event.preventDefault()
    const raw = event.dataTransfer?.getData('text/plain')
    const from = dragFromIndex.value ?? (raw ? Number(raw) : NaN)
    dragFromIndex.value = null
    dragOverIndex.value = null
    if (!Number.isInteger(from)) return
    moveTargetTo(from as number, index)
  }

  function onDragEnd(): void {
    dragFromIndex.value = null
    dragOverIndex.value = null
  }

  function onGripKeydown(event: KeyboardEvent, index: number): void {
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      moveTargetTo(index, index - 1)
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      moveTargetTo(index, index + 1)
    }
  }

  return {
    dragFromIndex,
    dragOverIndex,
    moveTargetTo,
    onRowDragStart,
    onRowDragOver,
    onDrop,
    onDragEnd,
    onGripKeydown,
  }
}
