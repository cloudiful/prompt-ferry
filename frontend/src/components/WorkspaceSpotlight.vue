<script setup lang="ts">
import { computed, useSlots } from 'vue'

const props = defineProps<{
  eyebrow?: string
  title: string
  description: string
  bullets?: string[]
  metricLabel?: string
  metricValue?: string | number
  metricHint?: string
  compact?: boolean
  minimal?: boolean
}>()

const slots = useSlots()

const hasMetric = computed(
  () => props.metricLabel || props.metricValue != null || props.metricHint,
)
const hasActions = computed(() => Boolean(slots.actions))
const hasAside = computed(() => hasMetric.value || hasActions.value)

const rootClass = computed(() => [
  props.compact
    ? 'gap-3 px-3 py-3 max-[767px]:gap-3 max-[767px]:p-3'
    : 'gap-4 px-4 py-4 max-[767px]:gap-3 max-[767px]:p-3',
  props.minimal
    ? 'max-[767px]:gap-1.5 max-[767px]:px-2.5 max-[767px]:py-2.5'
    : '',
  hasAside.value
    ? props.compact
      ? 'md:grid-cols-[minmax(0,1fr)_auto] md:items-center'
      : 'md:grid-cols-[minmax(0,1.35fr)_minmax(12rem,0.65fr)]'
    : '',
])

const contentClass = computed(() => (props.compact ? 'gap-1' : 'gap-2'))

const eyebrowClass = computed(() => (props.minimal ? 'max-[767px]:hidden' : ''))

const titleClass = computed(() =>
  props.compact
    ? 'text-[0.94rem] leading-[1.08] max-[767px]:text-[0.98rem]'
    : 'text-[1.05rem] leading-[1.15] max-[767px]:text-[0.98rem]',
)

const descriptionClass = computed(() =>
  props.compact
    ? 'text-[0.72rem] leading-[1.38] max-[767px]:text-[0.75rem]'
    : 'text-[0.8rem] leading-[1.6] max-[767px]:text-[0.75rem]',
)

const bulletsClass = computed(() =>
  props.compact
    ? 'mt-px gap-1 text-[0.72rem] leading-[1.5] max-[767px]:hidden'
    : 'mt-1 gap-1.5 text-[0.76rem] leading-[1.5] max-[767px]:text-[0.75rem]',
)

const asideClass = computed(() => [
  props.compact ? 'gap-3 max-[767px]:gap-0' : 'gap-3',
  props.minimal ? 'max-[767px]:hidden' : '',
])

const metricCardClass = computed(() =>
  props.compact ? 'gap-1 px-3 py-2.5 max-[767px]:hidden' : 'gap-1 p-4',
)

const metricValueClass = computed(() =>
  props.compact ? 'text-[1.18rem]' : 'text-[1.45rem]',
)

const metricHintClass = computed(() =>
  props.compact
    ? 'text-[0.74rem] leading-[1.45] max-[767px]:hidden'
    : 'text-[0.74rem] leading-[1.45] max-[767px]:text-[0.75rem]',
)
</script>

<template>
  <section
    class="grid grid-cols-1 items-start rounded-xl border border-default bg-default"
    :class="rootClass"
  >
    <div class="grid min-w-0" :class="contentClass">
      <div
        v-if="eyebrow"
        class="text-xs font-bold tracking-wide text-dimmed uppercase"
        :class="eyebrowClass"
      >
        {{ eyebrow }}
      </div>
      <h3 class="m-0 text-highlighted" :class="titleClass">
        {{ title }}
      </h3>
      <p class="m-0 text-muted" :class="descriptionClass">
        {{ description }}
      </p>

      <ul
        v-if="bullets?.length"
        class="grid pl-4 text-default"
        :class="bulletsClass"
      >
        <li v-for="item in bullets" :key="item">{{ item }}</li>
      </ul>
    </div>

    <div v-if="hasAside" class="grid justify-items-stretch" :class="asideClass">
      <div
        v-if="hasMetric"
        class="grid rounded-lg border border-default bg-default"
        :class="metricCardClass"
      >
        <div
          v-if="metricLabel"
          class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
        >
          {{ metricLabel }}
        </div>
        <div
          v-if="metricValue != null"
          class="text-highlighted font-bold leading-none"
          :class="metricValueClass"
        >
          {{ metricValue }}
        </div>
        <div v-if="metricHint" class="text-muted" :class="metricHintClass">
          {{ metricHint }}
        </div>
      </div>

      <div v-if="$slots.actions" class="grid gap-3">
        <slot name="actions" />
      </div>
    </div>
  </section>
</template>
