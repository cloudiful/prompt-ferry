import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'

const SHOW_ALL = Number.MAX_SAFE_INTEGER

export type UsagePreviewSlice = { hasMore: boolean; text: string }

/** The bounded preview of a long text, cutting at the first limit reached. */
export function usagePreviewText(
  text: string,
  level: number,
  stepChars: number,
  stepLines: number,
): UsagePreviewSlice {
  const normalized = text || ''
  if (!normalized) return { hasMore: false, text: '' }
  if (level >= SHOW_ALL) return { hasMore: false, text: normalized }
  const maxChars = stepChars * level
  const maxLines = stepLines * level
  let chars = 0
  let lines = 1
  let cut = normalized.length
  for (let index = 0; index < normalized.length; index += 1) {
    chars += 1
    if (chars > maxChars) {
      cut = index
      break
    }
    if (normalized[index] === '\n') {
      lines += 1
      if (lines > maxLines) {
        cut = index
        break
      }
    }
  }
  return {
    hasMore: cut < normalized.length,
    text: normalized.slice(0, cut).trimEnd(),
  }
}

type UsagePreviewTextProps = {
  allLabel: string
  collapseLabel: string
  emptyText: string
  maxHeight?: string
  moreLabel: string
  resetKey?: string | number
  stepChars?: number
  stepLines?: number
  text: string
  truncatedLabel: string
}

/**
 * A bounded, expandable text preview.
 *
 * The full text is always kept in memory and revealed in steps (or at once);
 * nothing is truncated away. The level resets when the dialog closes or the
 * record changes, which the parent signals through `resetKey`.
 */
export function UsagePreviewText({
  allLabel,
  collapseLabel,
  emptyText,
  maxHeight = '10rem',
  moreLabel,
  resetKey,
  stepChars = 12_000,
  stepLines = 160,
  text,
  truncatedLabel,
}: UsagePreviewTextProps) {
  const [level, setLevel] = useState(1)

  useEffect(() => {
    setLevel(1)
  }, [resetKey, text])

  if (!text) {
    return <div className="text-xs text-muted-foreground">{emptyText}</div>
  }

  const preview = usagePreviewText(text, level, stepChars, stepLines)
  const expanded = level > 1

  return (
    <div className="grid gap-2">
      <pre
        className="overflow-auto rounded border bg-muted px-3 py-2 text-xs break-words whitespace-pre-wrap"
        style={{ maxHeight }}
      >
        {preview.text}
      </pre>
      {(preview.hasMore || expanded) && (
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          {preview.hasMore && <span>{truncatedLabel}</span>}
          {preview.hasMore && (
            <Button
              onClick={() => setLevel((current) => current + 1)}
              size="xs"
              type="button"
              variant="ghost"
            >
              {moreLabel}
            </Button>
          )}
          {preview.hasMore && (
            <Button
              onClick={() => setLevel(SHOW_ALL)}
              size="xs"
              type="button"
              variant="ghost"
            >
              {allLabel}
            </Button>
          )}
          {expanded && (
            <Button
              onClick={() => setLevel(1)}
              size="xs"
              type="button"
              variant="ghost"
            >
              {collapseLabel}
            </Button>
          )}
        </div>
      )}
    </div>
  )
}
