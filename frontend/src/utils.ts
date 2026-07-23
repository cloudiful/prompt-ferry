export function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : []
}

export function appendQuery(
  query: URLSearchParams,
  key: string,
  value: string | null,
): void {
  if (value) query.set(key, value)
}

export function uniqueOptions(
  values: string[],
): Array<{ label: string; value: string }> {
  return [...new Set(values)]
    .filter(Boolean)
    .sort()
    .map((value) => ({ label: value, value }))
}
