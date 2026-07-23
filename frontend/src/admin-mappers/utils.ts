export function splitLines(text: string): string[] {
  return text
    .split(/\r?\n/g)
    .map((line) => line.trim())
    .filter(Boolean)
}

export function normalizeJsonArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : []
}

export function normalizeJsonRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {}
}

export function normalizeStringList(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : []
}

export function parseJsonText(
  text: string,
  fallback: unknown,
  errorMessage: string,
): unknown {
  const trimmed = text.trim()
  if (!trimmed) {
    return fallback
  }
  try {
    return JSON.parse(trimmed)
  } catch {
    throw new Error(errorMessage)
  }
}
