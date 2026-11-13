import path from 'node:path'
import { defineConfig } from '@hey-api/openapi-ts'

const codegenDir = import.meta.dirname

export default defineConfig({
  input: path.resolve(codegenDir, '../../../openapi/admin-api.yaml'),
  output: path.resolve(codegenDir, '../../src/generated/admin-api'),
  plugins: [
    '@hey-api/typescript',
    {
      name: '@hey-api/sdk',
      responseStyle: 'data',
    },
    '@hey-api/client-fetch',
    {
      name: '@tanstack/react-query',
      queryOptions: true,
      mutationOptions: true,
      queryKeys: true,
    },
  ],
})
