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
const THEME_PRIMARY = '#7455e9'
const THEME_PRIMARY_DARK = '#5b35c8'
const THEME_GRID = '#eeeaf7'
const THEME_MUTED = '#8b849b'

const yAxisLabelWidth = computed(() => {
  const values = props.data
    .map(point => point?.value?.[1])
    .filter((value): value is number => typeof value === 'number' && Number.isFinite(value))
    .slice(-30)
  const labels = values.map(value => formatValue(value))
  const longest = Math.max(4, ...labels.map(label => label.length))
  // Keep enough room for values such as "1024.0 MB/s" without making
  // the chart unusably narrow on small screens.
  return Math.min(112, Math.max(48, longest * 7 + 8))
})

watch(() => props.data, () => {
  chartRef.value?.setOption({
    series: [
      {
        data: props.data,
      },
    ],
  })
})

const option = computed<EChartsOption>(() => ({
  grid: {
    left: yAxisLabelWidth.value,
    right: 20,
    top: 10,
    bottom: 24,
  },
  tooltip: {
    trigger: 'axis',
    backgroundColor: 'rgba(255, 255, 255, 0.98)',
    borderColor: THEME_PRIMARY,
    borderWidth: 1,
    padding: [7, 10],
    textStyle: {
      color: '#312b3f',
      fontSize: 12,
    },
    extraCssText: 'border-radius: 8px; box-shadow: 0 8px 24px rgba(91, 53, 200, 0.14);',
    formatter: (params: any) => {
      params = params[0]
      return `<span style="color:${THEME_MUTED}">${formatTime(params.value[0])}</span><br><strong style="color:${THEME_PRIMARY_DARK};font-size:14px">${formatValue(params.value[1])}</strong>`
    },
    axisPointer: {
      animation: false,
      lineStyle: {
        color: '#b9a9f4',
        type: 'dashed',
        width: 1,
      },
    },
  },
  xAxis: {
    type: 'time',
    axisLine: {
      lineStyle: {
        color: '#dcd6e9',
      },
    },
    axisTick: {
      show: false,
    },
    splitLine: {
      show: false,
    },
    axisLabel: {
      color: THEME_MUTED,
      hideOverlap: true,
    },
  },
  yAxis: {
    type: 'value',
    min: props.max !== undefined ? 0 : undefined,
    max: (value: any) => {
      if (props.max !== undefined)
        return props.max
      return value.max === value.min ? value.max + 1 : undefined
    },
    interval: props.max !== undefined ? props.max / 4 : undefined,
    splitNumber: 4,
    axisLabel: {
      color: THEME_MUTED,
      hideOverlap: true,
      showMaxLabel: true,
      width: yAxisLabelWidth.value,
      overflow: 'break',
      formatter: (value: any) => formatValue(value),
    },
    axisLine: {
      show: false,
    },
    axisTick: {
      show: false,
    },
    splitLine: {
      lineStyle: {
        color: THEME_GRID,
        width: 1,
      },
    },
  },
  series: [
    {
      data: props.data,
      type: 'line',
      smooth: 0.22,
      connectNulls: true,
      showSymbol: true,
      symbol: 'circle',
      symbolSize: (_value: unknown, params: { dataIndex: number }) => params.dataIndex === props.data.length - 1 ? 7 : 0,
      lineStyle: {
        color: THEME_PRIMARY_DARK,
        width: 2.25,
      },
      itemStyle: {
        color: THEME_PRIMARY_DARK,
        borderColor: '#ffffff',
        borderWidth: 2,
      },
      areaStyle: {
        color: {
          type: 'linear',
          x: 0,
          y: 0,
          x2: 0,
          y2: 1,
          colorStops: [
            { offset: 0, color: 'rgba(116, 85, 233, 0.34)' },
            { offset: 0.62, color: 'rgba(116, 85, 233, 0.11)' },
            { offset: 1, color: 'rgba(116, 85, 233, 0.01)' },
          ],
        },
      },
      emphasis: {
        focus: 'series',
        lineStyle: {
          color: THEME_PRIMARY,
          width: 3,
        },
        itemStyle: {
          color: THEME_PRIMARY,
        },
      },
    },
  ],
  animationDuration: 420,
  animationEasing: 'cubicOut',
}))

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
  const index = Math.max(0, Math.min(Math.floor(Math.log(Math.abs(bytes)) / Math.log(1024)), units.length - 1))
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1)} ${units[index]}`
}
</script>

<style>
.chart {
  width: 100%;
  height: 100px;
}
</style>
