<template>
  <VChart class="chart" :style="{ height: `${height}px` }" :option="option" :autoresize="true" />
</template>

<script setup lang="ts">
import type { ComposeOption } from 'echarts/core'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import type { LineSeriesOption } from 'echarts/charts'
import { LineChart } from 'echarts/charts'
import type { GridComponentOption, LegendComponentOption, TooltipComponentOption } from 'echarts/components'
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
} from 'echarts/components'
import VChart from 'vue-echarts'
import type { ChartPoint, ChartSeries } from '@/history'

const props = withDefaults(defineProps<{
  data?: ChartPoint[]
  series?: ChartSeries[]
  unit: string
  decimals: number
  max?: number
  height?: number
}>(), {
  data: () => [],
  series: () => [],
  height: 100,
})

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
  | LegendComponentOption
  | LineSeriesOption
>

const THEME_PRIMARY = '#7455e9'
const THEME_PRIMARY_DARK = '#5b35c8'
const THEME_GRID = '#eeeaf7'
const THEME_MUTED = '#8b849b'
const SERIES_COLORS = ['#5b35c8', '#2997a8', '#d97706', '#db4b72', '#4f7fc9', '#7b8c2f']

const normalizedSeries = computed<ChartSeries[]>(() => props.series.length
  ? props.series
  : [{ name: '', data: props.data, color: THEME_PRIMARY_DARK }])
const showLegend = computed(() => normalizedSeries.value.length > 1)

const yAxisLabelWidth = computed(() => {
  const values = normalizedSeries.value
    .flatMap(series => series.data)
    .map(point => point?.value?.[1])
    .filter((value): value is number => typeof value === 'number' && Number.isFinite(value))
    .slice(-30)
  const labels = values.map(value => formatValue(value))
  const longest = Math.max(4, ...labels.map(label => label.length))
  // Keep enough room for values such as "1024.0 MB/s" without making
  // the chart unusably narrow on small screens.
  return Math.min(112, Math.max(48, longest * 7 + 8))
})

const option = computed<EChartsOption>(() => ({
  grid: {
    left: yAxisLabelWidth.value,
    right: 20,
    top: showLegend.value ? 34 : 10,
    bottom: 24,
  },
  legend: showLegend.value
    ? {
        top: 2,
        left: yAxisLabelWidth.value,
        right: 12,
        itemWidth: 16,
        itemHeight: 3,
        textStyle: { color: '#625b70', fontSize: 11 },
      }
    : undefined,
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
      const values = Array.isArray(params) ? params : [params]
      if (!values.length)
        return ''
      const lines = values.map((item: any) => {
        const label = showLegend.value ? `${item.marker}${item.seriesName}: ` : ''
        return `${label}<strong style="color:${item.color || THEME_PRIMARY_DARK}">${formatValue(item.value[1])}</strong>`
      })
      return `<span style="color:${THEME_MUTED}">${formatTime(values[0].value[0])}</span><br>${lines.join('<br>')}`
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
  series: normalizedSeries.value.map((series, index) => {
    const color = series.color || SERIES_COLORS[index % SERIES_COLORS.length]
    return {
      name: series.name,
      data: series.data.map(point => point.value),
      type: 'line',
      smooth: 0.22,
      connectNulls: true,
      showSymbol: true,
      symbol: 'circle',
      symbolSize: (_value: unknown, params: { dataIndex: number }) => params.dataIndex === series.data.length - 1 ? 7 : 0,
      lineStyle: {
        color,
        width: 2.25,
      },
      itemStyle: {
        color,
        borderColor: '#ffffff',
        borderWidth: 2,
      },
      areaStyle: showLegend.value
        ? undefined
        : {
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
          color,
          width: 3,
        },
        itemStyle: {
          color,
        },
      },
    } as LineSeriesOption
  }),
  animationDuration: 420,
  animationEasing: 'cubicOut',
}))

function formatTime(time: number) {
  const date = new Date(time)
  const years = date.getFullYear().toString()
  const months = (date.getMonth() + 1).toString().padStart(2, '0')
  const days = date.getDate().toString().padStart(2, '0')
  const hours = date.getHours().toString().padStart(2, '0')
  const minutes = date.getMinutes().toString().padStart(2, '0')
  const seconds = date.getSeconds().toString().padStart(2, '0')
  const timestamps = normalizedSeries.value.flatMap(series => series.data.map(point => point.value[0]))
  const span = timestamps.length ? Math.max(...timestamps) - Math.min(...timestamps) : 0
  if (span > 7 * 24 * 60 * 60 * 1000)
    return `${years}-${months}-${days}`
  if (span > 60 * 60 * 1000)
    return `${months}-${days} ${hours}:${minutes}`
  return `${hours}:${minutes}:${seconds}`
}

function formatValue(value: number) {
  if (!Number.isFinite(value))
    return '-'
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
}
</style>
