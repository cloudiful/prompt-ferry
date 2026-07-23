<script setup lang="ts">
defineProps<{
  title: string
  subtitle?: string
  inputId?: string
  card?: boolean
  dense?: boolean
}>()

const model = defineModel<boolean>({ required: true })
</script>

<template>
  <label
    :class="[
      'grid grid-cols-[minmax(0,1fr)_auto] items-center',
      card
        ? 'min-h-[4.5rem] rounded border border-default bg-default'
        : 'min-h-[3.25rem]',
      dense
        ? 'min-h-10 gap-2 px-2.5 py-2'
        : 'min-h-12 gap-4 px-3 py-3 max-[767px]:gap-3 max-[767px]:px-3 max-[767px]:py-2.5',
    ]"
  >
    <div class="min-w-0">
      <div
        v-if="title"
        class="text-sm font-semibold text-highlighted max-[767px]:text-[0.74rem] max-[767px]:leading-[1.12]"
      >
        {{ title }}
      </div>
      <div
        v-if="subtitle"
        class="text-xs text-dimmed max-[767px]:leading-[1.35]"
      >
        {{ subtitle }}
      </div>
    </div>
    <USwitch v-model="model" :id="inputId" />
  </label>
</template>
