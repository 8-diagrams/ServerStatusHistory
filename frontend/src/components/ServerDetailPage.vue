<template>
  <div class="min-h-screen bg-[#f7f7fb] text-[#24212d]">
    <header class="sticky top-0 z-50 border-b border-[#ebe8f2] bg-white/95 backdrop-blur">
      <div class="mx-auto flex max-w-[1800px] flex-wrap items-center justify-between gap-3 px-4 py-3 sm:px-6">
        <div class="flex min-w-0 items-center gap-3">
          <a href="#/" class="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-[#7455e9] text-xl font-bold text-white shadow-lg shadow-[#7455e9]/20">S</a>
          <div class="min-w-0">
            <h1 class="truncate text-xl font-bold tracking-tight sm:text-2xl">
              {{ server?.alias || host }}
            </h1>
            <p class="truncate text-xs text-gray-400">
              {{ t('serverDetails') }} · {{ host }}
            </p>
          </div>
        </div>
        <div class="flex flex-wrap items-center justify-end gap-2">
          <DropdownSelect
            v-model="rangeSeconds"
            :options="rangeOptions"
            :label="t('trendRange')"
            class="w-[132px]"
          />
          <DropdownSelect
            v-model="viewMode"
            :options="viewOptions"
            :label="t('chartMode')"
            class="w-[132px]"
          />
          <DropdownSelect
            v-model="locale"
            :options="languageOptions"
            :label="t('language')"
            variant="ghost"
            class="w-auto min-w-[92px]"
          />
          <a href="#/" class="rounded-lg border border-[#ded9ec] bg-white px-3 py-2 text-sm font-medium text-[#6544d5] hover:bg-[#f8f6ff]">← {{ t('backToOverview') }}</a>
        </div>
      </div>
    </header>

    <main class="mx-auto max-w-[1800px] p-4 sm:p-6">
      <div v-if="loading" class="rounded-2xl border border-[#ebe8f2] bg-white px-5 py-16 text-center text-gray-400">
        {{ t('loadingHosts') }}
      </div>
      <div v-else-if="!server" class="rounded-2xl border border-red-100 bg-white px-5 py-16 text-center text-red-500">
        {{ t('hostNotFound') }}
      </div>
      <template v-else>
        <section class="mb-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
          <div class="rounded-2xl border border-[#e9e6ef] bg-white px-4 py-3 shadow-sm">
            <div class="text-xs text-gray-400">
              {{ t('online') }}
            </div>
            <div class="mt-1 text-lg font-semibold" :class="isOnline(server) ? 'text-emerald-600' : 'text-red-500'">
              {{ isOnline(server) ? t('online') : t('offline') }}
            </div>
          </div>
          <div class="rounded-2xl border border-[#e9e6ef] bg-white px-4 py-3 shadow-sm">
            <div class="text-xs text-gray-400">
              {{ t('uptime') }}
            </div>
            <div class="mt-1 text-lg font-semibold">
              {{ isOnline(server) ? formatUptime(server.uptime) : t('offline') }}
            </div>
          </div>
          <div class="rounded-2xl border border-[#e9e6ef] bg-white px-4 py-3 shadow-sm">
            <div class="text-xs text-gray-400">
              {{ t('lastReportTime') }}
            </div>
            <div class="mt-1 text-sm font-semibold">
              {{ server.latest_ts ? formatTime(server.latest_ts, locale) : '-' }}
            </div>
          </div>
          <div class="rounded-2xl border border-[#e9e6ef] bg-white px-4 py-3 shadow-sm">
            <div class="text-xs text-gray-400">
              {{ t('directory') }}
            </div>
            <div class="mt-1 truncate text-lg font-semibold">
              {{ server.gid || t('defaultDirectory') }}
            </div>
          </div>
        </section>

        <div class="mb-3 flex items-center justify-between gap-3">
          <h2 class="text-lg font-bold">
            {{ viewMode === 'summary' ? t('summaryView') : t('detailedView') }}
          </h2>
          <span class="text-xs text-gray-400">{{ t('historyRefreshHint') }}</span>
        </div>
        <div class="grid gap-4 xl:grid-cols-2 2xl:grid-cols-3">
          <MetricChartPanel
            v-for="panel in visiblePanels"
            :key="panel.titleKey"
            :title="t(panel.titleKey)"
            :range-label="selectedRangeLabel"
            :metrics="panel.metrics"
            :histories="histories"
            :no-data-label="t('noHistoryData')"
          />
        </div>
      </template>
    </main>

    <footer class="border-t border-[#ebe8f2] bg-white py-4 text-center text-sm text-gray-400">
      {{ t('poweredBy') }} <a class="font-medium text-[#7455e9] hover:text-[#5b35c8]" href="https://github.com/8-diagrams/ServerStatusHistory" target="_blank" rel="noreferrer">ServerStatusHistory</a>
    </footer>
  </div>
</template>

<script setup lang="ts">
import type { MetricKey } from '@/history'
import { appendLiveMetrics, ensureMetricsHistory } from '@/history'
import { HISTORY_RANGES } from '@/historyRanges'
import type { ServerData } from '@/types'
import { formatTime, isOnline } from '@/utils'
import { formatUptime, useI18n } from '@/i18n'

const props = defineProps<{ host: string; gid?: string }>()
const { languageOptions, locale, t } = useI18n()
const rangeSeconds = ref(String(60 * 60))
const viewMode = ref<'summary' | 'detailed'>('summary')
const server = ref<ServerData>()
const histories = ref<Partial<Record<MetricKey, import('@/history').ChartPoint[]>>>({})
const loading = ref(true)
const pollTimer = ref<number>()

interface Panel { titleKey: import('@/i18n').MessageKey; metrics: MetricKey[] }
const summaryPanels: Panel[] = [
  { titleKey: 'panelCpu', metrics: ['cpu'] },
  { titleKey: 'panelLoad', metrics: ['load_1', 'load_5', 'load_15'] },
  { titleKey: 'panelMemory', metrics: ['memory_percent'] },
  { titleKey: 'panelDisk', metrics: ['hdd_percent'] },
  { titleKey: 'panelSwap', metrics: ['swap_percent'] },
  { titleKey: 'panelNetwork', metrics: ['network_rx', 'network_tx'] },
]
const detailedPanels: Panel[] = [
  ...summaryPanels,
  { titleKey: 'panelTraffic', metrics: ['traffic_in', 'traffic_out'] },
  { titleKey: 'panelConnections', metrics: ['tcp', 'udp'] },
  { titleKey: 'panelTasks', metrics: ['process', 'thread'] },
  { titleKey: 'panelCarrierLatency', metrics: ['time_10010', 'time_189', 'time_10086'] },
  { titleKey: 'panelCarrierLoss', metrics: ['ping_10010', 'ping_189', 'ping_10086'] },
  { titleKey: 'panelOnline', metrics: ['online4', 'online6'] },
]
const visiblePanels = computed(() => viewMode.value === 'summary' ? summaryPanels : detailedPanels)
const rangeOptions = computed(() => HISTORY_RANGES.map(range => ({ value: String(range.seconds), label: t(range.labelKey) })))
const viewOptions = computed(() => [
  { value: 'summary', label: t('summaryView') },
  { value: 'detailed', label: t('detailedView') },
])
const selectedRangeLabel = computed(() => rangeOptions.value.find(item => item.value === rangeSeconds.value)?.label || '')

async function fetchServer() {
  const params = new URLSearchParams({ host: props.host })
  if (props.gid !== undefined)
    params.set('gid', props.gid)
  try {
    const response = await fetch(`/api/hosts?${params}`)
    if (response.ok) {
      const data = await response.json() as { updated: number; servers: ServerData[] }
      server.value = data.servers?.find(item => item.name === props.host && (props.gid === undefined || item.gid === props.gid))
      if (server.value) {
        appendLiveMetrics(server.value, data.updated)
        return
      }
    }
  }
  catch {
    // Fall back to the legacy all-host feed for older servers.
  }
  try {
    const response = await fetch('/json/stats.json')
    if (!response.ok)
      return
    const data = await response.json() as { updated: number; servers: ServerData[] }
    server.value = data.servers?.find(item => item.name === props.host && (props.gid === undefined || item.gid === props.gid))
    if (server.value)
      appendLiveMetrics(server.value, data.updated)
  }
  catch {
    // Keep the previous server state visible during a transient poll failure.
  }
}

async function refreshHistory() {
  if (!server.value)
    return
  const metrics = [...new Set(visiblePanels.value.flatMap(panel => panel.metrics))]
  const result = await ensureMetricsHistory(server.value.name, metrics, Number(rangeSeconds.value), server.value.gid)
  histories.value = { ...histories.value, ...result }
}

async function refresh() {
  await fetchServer()
  await refreshHistory()
  loading.value = false
}

watch([rangeSeconds, viewMode], () => {
  refreshHistory().catch(() => undefined)
})
onMounted(async () => {
  await refresh()
  pollTimer.value = window.setInterval(() => {
    refresh().catch(() => undefined)
  }, 5000)
})
onUnmounted(() => {
  if (pollTimer.value)
    window.clearInterval(pollTimer.value)
})
</script>
