<template>
  <div
    class="rounded-xl border border-[#e9e6ef] bg-white px-4 py-3 relative shadow-sm transition-shadow hover:shadow-md"
  >
    <div class="absolute right-4 top-4 group flex flex-col items-end">
      <StatusIndicator
        :status="isOnline(server)"
        class="w-3 h-3"
      />
      <div class="hidden group-hover:block p-2 rounded-xl border text-sm bg-white z-[9999] mt-1 border-[#ded9ec] shadow-lg">
        <div class="flex gap-2">
          <div class="flex items-center gap-1">
            IPv4
            <StatusIndicator
              :status="server.online4"
              class="w-2 h-2"
            />
          </div>
          <div class="flex items-center gap-1">
            IPv6
            <StatusIndicator
              :status="server.online6"
              class="w-2 h-2"
            />
          </div>
        </div>
        <div v-if="server.latest_ts !== undefined">
          {{ t('lastReportTime') }}<br>
          {{ formatTime(server.latest_ts, locale) }}
        </div>
      </div>
    </div>
    <div class="text-lg flex items-center gap-2">
      <span v-if="isCountryFlagEmoji(server.location)">
        {{ server.location }}
      </span>
      <img
        v-else
        :src="`/static/flags/4x3/${server.location.toLowerCase()}.svg`" :alt="t('flagAlt', { location: server.location })"
        class="h-4 inline-block rounded-sm"
      >
      <img
        v-if="labels.os !== undefined"
        :src="`/static/os/${osIconName(labels.os)}.svg`" :alt="t('osAlt', { os: labels.os })"
        class="h-4 inline-block rounded-sm"
      >
      {{ server.alias || server.name }}
    </div>
    <div v-if="server.tags?.length" class="flex flex-wrap gap-1 my-1">
      <span
        v-for="tag in server.tags" :key="tag"
        class="text-xs bg-[#f1edff] text-[#6544d5] rounded-md px-2 py-0.5"
      >
        {{ tag }}
      </span>
    </div>
    <div>
      {{ t('uptime') }}
      <span
        :class="{
          'text-red-500': !isOnline(server),
        }"
      >
        {{ isOnline(server) ? formatUptime(server.uptime) : t('offline') }}
      </span>
    </div>
    <div v-if="!compactMode" class="flex items-center gap-2">
      {{ t('load') }}
      <Bandage v-if="noLoadData">
        {{ t('noData') }}
      </Bandage>
      <Bandage v-if="server.load !== undefined">
        {{ server.load }}
      </Bandage>
      <Bandage v-if="server.load_1 !== undefined">
        {{ server.load_1 }}
      </Bandage>
      <Bandage v-if="server.load_5 !== undefined">
        {{ server.load_5 }}
      </Bandage>
      <Bandage v-if="server.load_15 !== undefined">
        {{ server.load_15 }}
      </Bandage>
    </div>
    <div v-if="server.cpu !== undefined" class="flex items-center gap-2">
      CPU
      <Progress
        :value="server.cpu" :max="100"
        :text="`${server.cpu}%`"
        class="flex-1"
      >
        {{ server.cpu }}%
      </Progress>
    </div>
    <div v-if="server.memory_total !== undefined" class="flex items-center gap-2">
      {{ t('memory') }}
      <Progress
        :value="server.memory_used" :max="server.memory_total"
        class="flex-1"
      >
        {{ formatBytes(server.memory_used * 1024) }} / {{ formatBytes(server.memory_total * 1024) }}
      </Progress>
    </div>
    <div v-if="server.hdd_total !== undefined" class="flex items-center gap-2">
      {{ t('disk') }}
      <Progress
        :value="server.hdd_used" :max="server.hdd_total"
        class="flex-1"
      >
        {{ formatBytes(server.hdd_used * 1024 * 1024) }} / {{ formatBytes(server.hdd_total * 1024 * 1024) }}
      </Progress>
    </div>
    <div v-if="server.network_rx !== undefined" class="flex items-center gap-2">
      {{ t('network') }}
      <Bandage class="flex items-center">
        <IconDownload class="w-4 h-4" />{{ formatBytes(server.network_rx, 1) }}/s
      </Bandage>
      <Bandage class="flex items-center">
        <IconUpload class="w-4 h-4" />{{ formatBytes(server.network_tx, 1) }}/s
      </Bandage>
    </div>
    <div v-if="server.network_in !== undefined && !compactMode" class="flex items-center gap-2">
      {{ t('traffic') }}
      <Bandage class="flex items-center">
        <IconDownload class="w-4 h-4" />{{ formatBytes(server.network_in, 1) }}
      </Bandage>
      <Bandage class="flex items-center">
        <IconUpload class="w-4 h-4" />{{ formatBytes(server.network_out, 1) }}
      </Bandage>
    </div>
    <div v-if="server.swap_total !== undefined && !compactMode">
      SWAP
      <Bandage>
        {{ formatBytes(server.swap_used * 1024) }} / {{ formatBytes(server.swap_total * 1024) }}
      </Bandage>
    </div>
    <div
      v-if=" !compactMode"
      class="flex gap-1 flex-wrap mt-1"
    >
      <Bandage v-if="server.tcp_count !== undefined">
        TCP {{ server.tcp_count }}
      </Bandage>
      <Bandage v-if="server.udp_count !== undefined">
        UDP {{ server.udp_count }}
      </Bandage>
      <Bandage v-if="server.process_count !== undefined">
        {{ t('process') }} {{ server.process_count }}
      </Bandage>
      <Bandage v-if="server.thread_count !== undefined">
        {{ t('thread') }} {{ server.thread_count }}
      </Bandage>
    </div>
    <div v-if="showMetricChart" class="mt-3 border-t border-[#f0edf5] pt-2">
      <StatusChart
        :data="chartHistory"
        :unit="chartUnit"
        :decimals="chartDecimals"
        :max="chartMax"
      />
      <div class="mt-1 break-words text-center text-xs text-gray-500" :title="chartLabel">
        {{ chartLabel }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { ChartPoint } from '@/history'
import type { ServerData } from '@/types'
import { formatUptime, useI18n } from '@/i18n'
import { formatBytes, formatTime, hasLoadData, isCountryFlagEmoji, isOnline, osIconName, parseLabels } from '@/utils'

const props = defineProps<{
  server: ServerData
  compactMode: boolean
  showMetricChart: boolean
  chartHistory: ChartPoint[]
  chartLabel: string
  chartUnit: string
  chartDecimals: number
  chartMax?: number
}>()

const StatusChart = defineAsyncComponent(() => import('@/components/StatusChart.vue'))
const { locale, t } = useI18n()

const labels = computed(() => parseLabels(props.server.labels))
const noLoadData = computed(() => hasLoadData(props.server))
</script>
