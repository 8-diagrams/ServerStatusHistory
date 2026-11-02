<template>
  <VChart ref="chartRef" class="chart" :option="option" :autoresize="true" />
</template>

<script setup lang="ts">
import type { ComposeOption } from 'echarts/core'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import type { LineSeriesOption } from 'echarts/charts'
import { LineChart } from 'echarts/charts'
import type { GridComponentOption, TooltipComponentOption } from 'echarts/components'
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
} from 'echarts/components'
import VChart from 'vue-echarts'

const props = defineProps<{
  data: any[]
  unit: string
  decimals: number
  max?: number
}>()

use([
  CanvasRenderer,
  LineChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
])

type EChartsOption = ComposeOption<
  | TooltipComponentOption
  | GridComponentOption
  | LineSeriesOption
>

const chartRef = ref<any>()

watch(() => props.data, () => {
  chartRef.value?.setOption({
    series: [
      {
        data: props.data,
      },
    ],
  })
})

const option: EChartsOption = {
  grid: {
    left: 40,
    right: 20,
    top: 10,
    bottom: 20,
  },
  tooltip: {
    trigger: 'axis',
    formatter: (params: any) => {
      params = params[0]
      return `${formatTime(params.value[0])}: ${formatValue(params.value[1])}`
    },
    axisPointer: {
      animation: false,
    },
  },
  xAxis: {
    type: 'time',
    splitLine: {
      show: false,
    },
    axisLabel: {
      hideOverlap: true,
    },
  },
  yAxis: {
    type: 'value',
    max: (value: any) => {
      if (props.max !== undefined)
        return props.max
      return value.max === value.min ? value.max + 1 : undefined
    },
    axisLabel: {
      hideOverlap: true,
      showMaxLabel: true,
      formatter: (value: any) => formatValue(value),
    },
  },
  series: [
    {
      data: props.data,
      type: 'line',
      showSymbol: false,
    },
  ],
}

function formatTime(time: number) {
  const date = new Date(time)
  const hours = date.getHours().toString().padStart(2, '0')
  const minutes = date.getMinutes().toString().padStart(2, '0')
  const seconds = date.getSeconds().toString().padStart(2, '0')
  return `${hours}:${minutes}:${seconds}`
}

function formatValue(value: number) {
  if (props.unit === 'B' || props.unit === 'B/s')
    return `${formatBytes(value)}${props.unit === 'B/s' ? '/s' : ''}`
  return `${value.toFixed(props.decimals)}${props.unit}`
}

function formatBytes(bytes: number) {
  if (bytes === 0)
    return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
  const index = Math.min(Math.floor(Math.log(Math.abs(bytes)) / Math.log(1024)), units.length - 1)
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`
}
</script>

<style>
.chart {
  width: 100%;
  height: 100px;
}
</style>
