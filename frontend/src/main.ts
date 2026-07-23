import ui from '@nuxt/ui/vue-plugin'
import 'virtual:nuxt-icon-bundle/register'
import { createApp } from 'vue'
import App from './App.vue'
import './api'
import { i18n } from './i18n/plugin'
import { pinia } from './pinia'
import { router } from './router'
import './style.css'
import { initTheme } from './theme/appTheme'

initTheme()

const app = createApp(App)

app.use(i18n).use(pinia).use(router).use(ui)

app.mount('#app')

// Keep the initial navigation asynchronous, but do not block the whole app
// from mounting. A stalled ready promise leaves the page permanently blank.
void router.isReady()
