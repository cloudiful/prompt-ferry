import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import path from 'node:path'
import { defineConfig } from 'vite'

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
    },
  },
  plugins: [react(), tailwindcss()],
  server: {
    port: 5171,
    proxy: {
      '/api/': 'http://127.0.0.1:8789',
    },
    watch: {
      ignored: ['**/.playwright-cli/**'],
    },
  },
})
