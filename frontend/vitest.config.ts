import path from 'node:path'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  test: {
    environment: 'jsdom',
    include: [
      'src/admin-mappers/forms/**/*.test.ts',
      'src/react-tests/**/*.test.ts',
      'src/react-tests/**/*.test.tsx',
    ],
    setupFiles: ['src/react-tests/setup.ts'],
  },
})
