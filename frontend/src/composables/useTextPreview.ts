import { ref, type Ref } from 'vue'

export type TruncatedTextPreview = {
  hasMore: boolean
  text: string
}

const PREVIEW_SHOW_ALL = Number.MAX_SAFE_INTEGER

export function previewText(
  text: string,
  level: number,
  stepChars: number,
  stepLines: number,
): TruncatedTextPreview {
  const normalized = text || ''
  if (!normalized) {
    return { hasMore: false, text: '' }
  }
  if (level >= PREVIEW_SHOW_ALL) {
    return { hasMore: false, text: normalized }
  }
  const maxChars = stepChars * level
  const maxLines = stepLines * level
  const cutIndex = previewCutIndex(normalized, maxChars, maxLines)
  return {
    hasMore: cutIndex < normalized.length,
    text: normalized.slice(0, cutIndex).trimEnd(),
  }
}

export function createPreviewLevel(initial = 1): Ref<number> {
  return ref(initial)
}

export function resetPreviewLevels(...targets: Ref<number>[]): void {
  targets.forEach((target) => {
    target.value = 1
  })
}

export function showMorePreview(target: Ref<number>, amount = 1): void {
  target.value += amount
}

export function showAllPreview(target: Ref<number>): void {
  target.value = PREVIEW_SHOW_ALL
}

export function collapsePreview(target: Ref<number>): void {
  target.value = 1
}

function previewCutIndex(
  text: string,
  maxChars: number,
  maxLines: number,
): number {
  let chars = 0
  let lines = 1
  for (let index = 0; index < text.length; index += 1) {
    chars += 1
    if (chars > maxChars) return index
    if (text[index] === '\n') {
      lines += 1
      if (lines > maxLines) return index
    }
  }
  return text.length
}
