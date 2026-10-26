import path from 'node:path'
import { defineConfig } from '@hey-api/openapi-ts'

const codegenDir = import.meta.dirname

export default defineConfig({
  input: path.resolve(codegenDir, '../../../openapi/admin-api.yaml'),
  output: path.resolve(codegenDir, '../../src/generated/admin-api'),
  plugins: ['@hey-api/typescript', '@hey-api/sdk', '@hey-api/client-fetch'],
})
