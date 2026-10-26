import { useEffect, useRef, useState } from 'react'

export function reorderTargetByIdentity<T>(
  targets: readonly T[],
  source: T,
  destination: T,
): T[] | null {
  const from = targets.indexOf(source)
  const to = targets.indexOf(destination)
  if (from < 0 || to < 0) return null
  if (from === to) return [...targets]
  const next = [...targets]
  const [target] = next.splice(from, 1)
  if (target === undefined) return null
  next.splice(to, 0, target)
  return next
}

export function useModelRouteTargetDrag<T>(
  targets: readonly T[],
  onReorder: (targets: T[]) => void,
) {
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null)
  const dragTarget = useRef<T | null>(null)

  useEffect(() => {
    if (dragTarget.current !== null && !targets.includes(dragTarget.current)) {
      dragTarget.current = null
      setDragOverIndex(null)
    }
  }, [targets])

  function onDragStart(event: React.DragEvent, target: T): void {
    const element = event.target as HTMLElement | null
    if (!element?.closest('[data-target-drag-handle]')) {
      event.preventDefault()
      return
    }
    if (!targets.includes(target)) {
      event.preventDefault()
      return
    }
    dragTarget.current = target
    if (event.dataTransfer) {
      event.dataTransfer.setData('text/plain', 'model-route-target')
      event.dataTransfer.effectAllowed = 'move'
    }
  }

  function onDragOver(event: React.DragEvent, target: T): void {
    if (!targets.includes(target)) return
    event.preventDefault()
    setDragOverIndex(targets.indexOf(target))
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
  }

  function clearDrag(): void {
    dragTarget.current = null
    setDragOverIndex(null)
  }

  function onDrop(event: React.DragEvent, destination: T): void {
    event.preventDefault()
    const source = dragTarget.current
    clearDrag()
    if (source === null) return
    const next = reorderTargetByIdentity(targets, source, destination)
    if (next && next.some((target, index) => target !== targets[index]))
      onReorder(next)
  }

  function onDragEnd(): void {
    clearDrag()
  }

  function onGripKeyDown(event: React.KeyboardEvent, target: T): void {
    if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
    const index = targets.indexOf(target)
    const nextIndex = event.key === 'ArrowUp' ? index - 1 : index + 1
    if (index < 0 || nextIndex < 0 || nextIndex >= targets.length) return
    event.preventDefault()
    const next = [...targets]
    const [item] = next.splice(index, 1)
    if (item === undefined) return
    next.splice(nextIndex, 0, item)
    onReorder(next)
  }

  return {
    dragOverIndex,
    onDragEnd,
    onDragOver,
    onDragStart,
    onDrop,
    onGripKeyDown,
  }
}
