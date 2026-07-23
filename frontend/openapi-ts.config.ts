import { defineConfig } from '@hey-api/openapi-ts'

export default defineConfig({
  input: '../openapi/admin-api.yaml',
  output: 'src/generated/admin-api',
  plugins: ['@hey-api/typescript', '@hey-api/sdk', '@hey-api/client-fetch'],
})
