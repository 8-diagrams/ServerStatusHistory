<template>
  <section class="rounded-2xl border border-[#e9e6ef] bg-white p-4 shadow-sm">
    <div class="mb-2">
      <h3 class="truncate text-sm font-semibold text-[#312b3f]">
        {{ title }} ({{ rangeLabel }})
      </h3>
    </div>
    <StatusChart
      v-if="series.length && series.some(item => item.data.length)"
      :series="series"
      :unit="unit"
      :decimals="decimals"
      :max="max"
      :height="220"
    />
    <div v-else class="flex h-[220px] items-center justify-center rounded-xl bg-[#faf9fd] text-sm text-gray-400">
      {{ noDataLabel }}
    </div>
  </section>
</template>

<script setup lang="ts">
import type { ChartPoint, ChartSeries, MetricKey } from '@/history'
import { metricOption } from '@/history'
import { useI18n } from '@/i18n'

const props = defineProps<{
  title: string
  rangeLabel: string
  metrics: MetricKey[]
  histories: Partial<Record<MetricKey, ChartPoint[]>>
  noDataLabel: string
}>()
const { t } = useI18n()

const series = computed<ChartSeries[]>(() => props.metrics.map(metric => ({
  name: t(metricOption(metric).labelKey),
  data: props.histories[metric] || [],
})))
const firstOption = computed(() => metricOption(props.metrics[0] || 'cpu'))
const unit = computed(() => firstOption.value.unit)
const decimals = computed(() => firstOption.value.decimals)
const max = computed(() => {
  const values = props.metrics.map(metric => metricOption(metric).max)
  return values.every(value => value !== undefined && value === values[0]) ? values[0] : undefined
})
</script>
