<template>
  <div v-if="visibleCarriers.length" class="grid grid-cols-3 gap-0.5">
    <div
      v-for="carrier in visibleCarriers"
      :key="carrier.name"
      class="flex min-w-0 items-center justify-center gap-0.5 whitespace-nowrap rounded-full px-1 py-0.5 text-[10px] leading-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.20)]"
      :class="{
        'bg-[#166534] text-white': carrier.loss <= 1,
        'bg-[#bbf7d0] text-[#14532d]': carrier.loss > 1 && carrier.loss <= 2,
        'bg-[#fef08a] text-[#713f12]': carrier.loss > 2 && carrier.loss < 5,
        'bg-[#fed7aa] text-[#9a3412]': carrier.loss >= 5 && carrier.loss < 25,
        'bg-[#991b1b] text-white': carrier.loss >= 25,
      }"
      :title="carrier.title"
    >
      <span class="font-bold">{{ carrier.name }}</span>
      <span>{{ formatNumber(carrier.latency) }}ms</span>
      <span class="opacity-55">·</span>
      <span class="font-bold">
        {{ formatNumber(carrier.loss) }}%
      </span>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { ServerData } from '@/types'
import { useI18n } from '@/i18n'

const props = defineProps<{
  server: ServerData
}>()

const { t } = useI18n()

const visibleCarriers = computed(() => [
  {
    name: t('carrierUnicom'),
    latency: props.server.time_10010,
    loss: props.server.ping_10010,
    title: `${t('metricUnicomLatency')} / ${t('metricUnicomLoss')}`,
  },
  {
    name: t('carrierTelecom'),
    latency: props.server.time_189,
    loss: props.server.ping_189,
    title: `${t('metricTelecomLatency')} / ${t('metricTelecomLoss')}`,
  },
  {
    name: t('carrierMobile'),
    latency: props.server.time_10086,
    loss: props.server.ping_10086,
    title: `${t('metricMobileLatency')} / ${t('metricMobileLoss')}`,
  },
].filter(carrier => Number.isFinite(carrier.latency)
  && Number.isFinite(carrier.loss)
  && (carrier.latency > 0 || carrier.loss > 0)))

function formatNumber(value: number) {
  return Number.isInteger(value) ? String(value) : value.toFixed(1)
}
</script>
