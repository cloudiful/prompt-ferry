export const ENDPOINT_MODEL_CATALOG_TTL_MS = 60_000

export function normalizeEndpointModels(models: readonly unknown[]): string[] {
  return [
    ...new Set(
      models
        .filter(
          (model): model is string =>
            typeof model === 'string' && model.trim() !== '',
        )
        .map((model) => model.trim()),
    ),
  ].sort((a, b) => a.localeCompare(b))
}
