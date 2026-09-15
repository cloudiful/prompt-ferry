<script setup lang="ts">
import { computed, ref, watch } from 'vue'

type ProxyScheme = 'direct' | 'http' | 'https' | 'socks5' | 'socks5h' | 'kept'

const props = defineProps<{
  hint: string
  initialValue: string
  hasSaved: boolean
  t: TranslateFn
}>()

const emit = defineEmits<{
  save: [value: string]
  clear: []
  cancel: []
}>()

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

function composeProxyUrl(scheme: ProxyScheme, address: string): string {
  const trimmed = (address ?? '').trim()
  if (scheme === 'direct' || scheme === 'kept') return ''
  if (!trimmed) return ''
  if (trimmed.includes('://')) return trimmed
  return `${scheme}://${trimmed}`
}

const scheme = ref<ProxyScheme>('direct')
const address = ref('')
const initialScheme = ref<ProxyScheme>('direct')
const initialAddress = ref('')
const schemeTouched = ref(false)
const addressTouched = ref(false)

function resetFromProps(): void {
  const parsed = parseProxyUrl(props.initialValue ?? '')
  // When the server holds a masked value there is nothing to parse;
  // show a neutral kept option so the stored proxy stays hidden without
  // claiming direct. Saving without touching keeps the stored value;
  // only an explicit clear action or an explicit direct selection clears it.
  if (!props.initialValue?.trim() && props.hasSaved) {
    scheme.value = 'kept'
    address.value = ''
  } else {
    scheme.value = parsed.scheme
    address.value = parsed.scheme === 'direct' ? '' : parsed.address
  }
  initialScheme.value = scheme.value
  initialAddress.value = address.value
  schemeTouched.value = false
  addressTouched.value = false
}

resetFromProps()

watch(
  () => [props.initialValue, props.hasSaved] as const,
  () => {
    resetFromProps()
  },
)

const schemeItems = computed(() => {
  const base = PROXY_SCHEMES.map((value) => ({
    label: value === 'direct' ? props.t('proxyDirect') : value,
    value,
  }))
  if (initialScheme.value === 'kept' || scheme.value === 'kept') {
    return [
      { label: props.t('proxySchemeKept'), value: 'kept' as ProxyScheme },
      ...base,
    ]
  }
  return base
})

const addressPlaceholder = computed(() => {
  switch (scheme.value) {
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

const canSave = computed(() => {
  if (scheme.value === 'direct' || scheme.value === 'kept') return true
  return address.value.trim() !== ''
})

const isDirty = computed(
  () =>
    schemeTouched.value ||
    addressTouched.value ||
    scheme.value !== initialScheme.value ||
    address.value.trim() !== initialAddress.value.trim(),
)

function onSchemeUpdate(value: ProxyScheme): void {
  scheme.value = value
  schemeTouched.value = true
}

function onAddressUpdate(value: string): void {
  address.value = value
  addressTouched.value = true
}

function onSave(): void {
  // Untouched save keeps the stored proxy: emit cancel so the parent stays
  // on `proxy_url="" + has_saved=true` (request omits the key).
  // Explicit direct selection or typing a value marks dirty and emits.
  // The neutral kept option always means keep, even when touched.
  if (props.hasSaved && (!isDirty.value || scheme.value === 'kept')) {
    emit('cancel')
    return
  }
  emit('save', composeProxyUrl(scheme.value, address.value))
}

function onClear(): void {
  emit('clear')
}

function onCancel(): void {
  emit('cancel')
}
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
        :model-value="scheme"
        class="w-full"
        :items="schemeItems"
        label-key="label"
        value-key="value"
        @update:model-value="onSchemeUpdate($event as ProxyScheme)"
      />
    </label>
    <label v-if="scheme !== 'direct' && scheme !== 'kept'" class="grid gap-1">
      <span class="text-xs text-muted">{{ t('proxyAddress') }}</span>
      <UInput
        :model-value="address"
        class="w-full"
        :placeholder="addressPlaceholder"
        @update:model-value="onAddressUpdate($event as string)"
      />
    </label>
    <p class="text-xs leading-snug text-muted">{{ hint }}</p>
    <p v-if="hasSaved" class="text-xs leading-snug text-muted">
      {{ t('proxyKeepHint') }}
    </p>
    <div class="flex justify-end gap-2 pt-1">
      <UButton
        type="button"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="onClear"
        >{{ t('proxyClear') }}</UButton
      >
      <UButton
        type="button"
        size="sm"
        color="neutral"
        variant="outline"
        @click="onCancel"
        >{{ t('cancel') }}</UButton
      >
      <UButton type="button" size="sm" :disabled="!canSave" @click="onSave">{{
        t('save')
      }}</UButton>
    </div>
  </div>
</template>
