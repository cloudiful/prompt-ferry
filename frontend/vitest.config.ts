import { defineConfig } from 'vitest/config'

// #569 Phase 4: Vitest runs only the new admin-mapper form tests. The existing
// frontend/tests suites stay on `bun:test` and must not be migrated or picked
// up here.
export default defineConfig({
  test: {
    include: ['src/admin-mappers/forms/**/*.test.ts'],
  },
})
