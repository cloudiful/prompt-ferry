import type { RedactionConfigSchema } from '../generated/admin-api'

export function createRedactionDefaults(
  config?: RedactionConfigSchema,
): RedactionConfigSchema {
  return (
    config ?? {
      enabled: false,
      rules: {
        secret: true,
        domain: true,
        url: true,
        email: true,
        ip: true,
        cidr: true,
        phone: true,
        person: true,
        organization: true,
      },
      custom_strings: [],
    }
  )
}
