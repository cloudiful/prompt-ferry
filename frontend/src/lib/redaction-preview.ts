import type { RedactionFindingSchema } from '@/generated/admin-api'

export type RedactionPreviewSegment = {
  text: string
  highlighted: boolean
}

function plainSegment(text: string): RedactionPreviewSegment[] {
  return [{ text, highlighted: false }]
}

export function redactionInputSegments(
  text: string,
  finding: RedactionFindingSchema | null,
): RedactionPreviewSegment[] {
  if (
    !finding ||
    typeof finding.start !== 'number' ||
    !finding.match_text ||
    !text.includes(finding.match_text)
  ) {
    return plainSegment(text)
  }

  const hitIndex = text.indexOf(finding.match_text, finding.start)
  if (hitIndex < 0) return plainSegment(text)

  const segments: RedactionPreviewSegment[] = []
  const before = text.slice(0, hitIndex)
  const after = text.slice(hitIndex + finding.match_text.length)
  if (before) segments.push({ text: before, highlighted: false })
  segments.push({ text: finding.match_text, highlighted: true })
  if (after) segments.push({ text: after, highlighted: false })
  return segments
}

export function redactionOutputSegments(
  text: string,
  finding: RedactionFindingSchema | null,
): RedactionPreviewSegment[] {
  if (!finding) return plainSegment(text)

  const { start, end } = finding
  if (
    typeof start !== 'number' ||
    typeof end !== 'number' ||
    start < 0 ||
    end <= start ||
    start >= text.length
  ) {
    return plainSegment(text)
  }

  const clampedEnd = Math.min(end, text.length)
  const segments: RedactionPreviewSegment[] = []
  const before = text.slice(0, start)
  const hit = text.slice(start, clampedEnd)
  const after = text.slice(clampedEnd)
  if (before) segments.push({ text: before, highlighted: false })
  if (hit) segments.push({ text: hit, highlighted: true })
  if (after) segments.push({ text: after, highlighted: false })
  return segments
}
