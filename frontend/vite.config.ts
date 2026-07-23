import tailwindcss from '@tailwindcss/vite'
import { NuxtIconBundle } from '@nuxt/icon/vite'
import ui from '@nuxt/ui/vite'
import vue from '@vitejs/plugin-vue'
import path from 'node:path'
import { defineConfig } from 'vite'

const bundledLucideIcons = [
  'activity',
  'arrow-down',
  'arrow-left',
  'arrow-right',
  'arrow-up',
  'arrow-up-right',
  'blocks',
  'cable',
  'chart-column',
  'chart-no-axes-column',
  'check',
  'chevron-down',
  'chevron-left',
  'chevron-right',
  'chevron-up',
  'chevrons-left',
  'chevrons-right',
  'circle-alert',
  'circle-check',
  'circle-x',
  'clipboard-check',
  'clipboard-list',
  'clock',
  'copy',
  'copy-check',
  'database',
  'database-zap',
  'ellipsis',
  'eye',
  'eye-off',
  'file',
  'folder',
  'folder-open',
  'grip-vertical',
  'hash',
  'info',
  'key',
  'key-round',
  'lightbulb',
  'loader-circle',
  'log-in',
  'log-out',
  'menu',
  'minus',
  'monitor',
  'moon',
  'network',
  'panel-left-close',
  'panel-left-open',
  'pencil',
  'plug',
  'plug-zap',
  'plus',
  'refresh-cw',
  'rotate-ccw',
  'save',
  'search',
  'server',
  'settings',
  'settings-2',
  'shield',
  'shield-check',
  'square',
  'sun',
  'trash-2',
  'triangle-alert',
  'upload',
  'users',
  'users-round',
  'workflow',
  'x',
]

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  plugins: [
    vue(),
    tailwindcss(),
    NuxtIconBundle({
      icons: bundledLucideIcons.map((name) => `lucide:${name}`),
    }),
    ui({
      ui: {
        colors: {
          primary: 'blue',
          neutral: 'slate',
        },
      },
    }),
  ],
  server: {
    port: 5171,
    proxy: {
      '/api': 'http://127.0.0.1:8789',
    },
  },
})
