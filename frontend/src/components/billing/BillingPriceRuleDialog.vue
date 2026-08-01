<script setup lang="ts">
import type { BillingPriceRuleForm } from '@/models/billing'

defineProps<{
  busy: boolean
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<BillingPriceRuleForm>('form', { required: true })

defineEmits<{
  save: []
}>()
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="form.price_rule_id ? t('editPriceRule') : t('newPriceRule')"
    :ui="{ content: 'sm:max-w-3xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
        <div class="grid gap-2">
          <UFormField :label="t('effectiveFrom')">
            <UInput
              v-model="form.effective_from"
              type="datetime-local"
              class="w-full"
              required
            />
          </UFormField>
        </div>
        <UFormField :label="t('publicModel')">
          <UInput v-model="form.public_model" class="w-full" required />
        </UFormField>
        <div class="grid gap-2 sm:grid-cols-2">
          <UFormField :label="`${t('inputRate')} (${t('perMillionTokens')})`"
            ><UInput
              v-model="form.input_rate"
              class="w-full"
              inputmode="decimal"
              required
          /></UFormField>
          <UFormField
            :label="`${t('cacheReadRate')} (${t('perMillionTokens')})`"
            ><UInput
              v-model="form.cache_read_rate"
              class="w-full"
              inputmode="decimal"
              required
          /></UFormField>
          <UFormField
            :label="`${t('cacheWriteRate')} (${t('perMillionTokens')})`"
            ><UInput
              v-model="form.cache_write_rate"
              class="w-full"
              inputmode="decimal"
              required
          /></UFormField>
          <UFormField :label="`${t('outputRate')} (${t('perMillionTokens')})`"
            ><UInput
              v-model="form.output_rate"
              class="w-full"
              inputmode="decimal"
              required
          /></UFormField>
        </div>
        <p class="text-xs text-dimmed">{{ t('billingRateHint') }}</p>
        <div class="flex justify-end gap-2">
          <UButton
            type="button"
            color="neutral"
            variant="outline"
            @click="visible = false"
            >{{ t('cancel') }}</UButton
          >
          <UButton type="submit" :loading="busy" icon="i-lucide-save">{{
            t('save')
          }}</UButton>
        </div>
      </form>
    </template>
  </UModal>
</template>
