<script setup lang="ts">
import { computed } from 'vue'

type ProxyScheme = 'direct' | 'http' | 'https' | 'socks5' | 'socks5h' | 'kept'

const props = defineProps<{
  hint: string
  t: TranslateFn
}>()

const proxyUrl = defineModel<string>('proxyUrl', { required: true })
const hasSaved = defineModel<boolean>('hasSaved', { required: true })

const PROXY_SCHEMES: ProxyScheme[] = [
  'direct',
  'http',
  'https',
  'socks5',
  'socks5h',
]

function parseProxyUrl(value: string): {
  scheme: ProxyScheme
  address: string
} {
  const trimmed = (value ?? '').trim()
  if (!trimmed) return { scheme: 'direct', address: '' }
  const match = trimmed.match(/^(http|https|socks5h|socks5):\/\/(.*)$/i)
  if (match) {
    const scheme = match[1].toLowerCase() as ProxyScheme
    return { scheme, address: (match[2] ?? '').trim() }
  }
  return { scheme: 'http', address: trimmed }
}

const currentScheme = computed<ProxyScheme>(() => {
  if (!proxyUrl.value?.trim() && hasSaved.value) return 'kept'
  return parseProxyUrl(proxyUrl.value ?? '').scheme
})

const currentAddress = computed<string>(() => {
  if (currentScheme.value === 'direct' || currentScheme.value === 'kept')
    return ''
  return parseProxyUrl(proxyUrl.value ?? '').address
})

const schemeItems = computed(() => {
  const base = PROXY_SCHEMES.map((value) => ({
    label: value === 'direct' ? props.t('proxyDirect') : value,
    value,
  }))
  if (currentScheme.value === 'kept') {
    return [
      { label: props.t('proxySchemeKept'), value: 'kept' as ProxyScheme },
      ...base,
    ]
  }
  if (hasSaved.value) {
    return [
      { label: props.t('proxySchemeKept'), value: 'kept' as ProxyScheme },
      ...base,
    ]
  }
  return base
})

const addressPlaceholder = computed(() => {
  switch (currentScheme.value) {
    case 'kept':
      return ''
    case 'https':
      return 'https://127.0.0.1:8443'
    case 'socks5':
      return 'socks5://user:pass@10.0.0.1:1080'
    case 'socks5h':
      return 'socks5h://user:pass@10.0.0.1:1080'
    default:
      return 'http://127.0.0.1:7890'
  }
})

function onSchemeUpdate(value: ProxyScheme): void {
  if (value === 'kept') {
    proxyUrl.value = ''
    hasSaved.value = true
    return
  }
  if (value === 'direct') {
    proxyUrl.value = ''
    hasSaved.value = false
    return
  }
  const addr = currentAddress.value.trim()
  if (!addr) {
    proxyUrl.value = `${value}://`
    return
  }
  if (addr.includes('://')) {
    proxyUrl.value = addr
    return
  }
  proxyUrl.value = `${value}://${addr}`
}

function onAddressUpdate(value: string): void {
  const scheme = currentScheme.value
  if (scheme === 'direct' || scheme === 'kept') return
  const trimmed = (value ?? '').trim()
  if (!trimmed) {
    proxyUrl.value = `${scheme}://`
    return
  }
  if (trimmed.includes('://')) {
    proxyUrl.value = trimmed
    return
  }
  proxyUrl.value = `${scheme}://${trimmed}`
}

function onClear(): void {
  proxyUrl.value = ''
  hasSaved.value = false
}

const showClear = computed(
  () => hasSaved.value || (proxyUrl.value ?? '').trim() !== '',
)
</script>

<template>
  <div class="grid gap-3 text-xs">
    <div v-if="hasSaved" class="flex items-center gap-2">
      <UBadge :label="t('saved')" color="neutral" />
      <span class="text-muted">{{ t('proxySavedHidden') }}</span>
    </div>
    <label class="grid gap-1">
      <span class="text-xs text-muted">{{ t('proxyScheme') }}</span>
      <USelect
        :model-value="currentScheme"
        class="w-full"
        :items="schemeItems"
        label-key="label"
        value-key="value"
        @update:model-value="onSchemeUpdate($event as ProxyScheme)"
      />
    </label>
    <label
      v-if="currentScheme !== 'direct' && currentScheme !== 'kept'"
      class="grid gap-1"
    >
      <span class="text-xs text-muted">{{ t('proxyAddress') }}</span>
      <UInput
        :model-value="currentAddress"
        class="w-full"
        :placeholder="addressPlaceholder"
        @update:model-value="onAddressUpdate($event as string)"
      />
    </label>
    <p class="text-xs leading-snug text-muted">{{ hint }}</p>
    <p v-if="hasSaved" class="text-xs leading-snug text-muted">
      {{ t('proxyKeepHint') }}
    </p>
    <div v-if="showClear" class="flex justify-start">
      <UButton
        type="button"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="onClear"
        >{{ t('proxyClear') }}</UButton
      >
    </div>
  </div>
</template>
