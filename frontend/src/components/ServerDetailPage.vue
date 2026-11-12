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
            v-model="rangeSelection"
            :options="rangeOptions"
            :label="t('trendRange')"
            class="w-[146px]"
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
        <section class="mb-5 rounded-2xl border border-[#e9e6ef] bg-white p-4 shadow-sm">
          <div class="flex flex-wrap items-end gap-3">
            <div v-if="isCustomRange" class="grid min-w-[280px] flex-1 gap-3 sm:grid-cols-2">
              <label class="block text-xs font-medium text-[#625b70]">
                {{ t('historyStart') }}
                <input
                  v-model="customFrom"
                  type="datetime-local"
                  class="mt-1 w-full rounded-xl border border-[#ded9ec] bg-[#faf9fd] px-3 py-2 text-sm text-[#312b3f] outline-none focus:border-[#7455e9] focus:ring-2 focus:ring-[#7455e9]/15"
                >
              </label>
              <div class="block text-xs font-medium text-[#625b70]">
                {{ t('historyEnd') }}
                <div class="mt-1 flex rounded-xl bg-[#f5f2ff] p-1">
                  <button
                    type="button"
                    class="flex-1 rounded-lg px-3 py-1.5 text-sm transition-colors"
                    :class="customEndMode === 'now' ? 'bg-white font-semibold text-[#6544d5] shadow-sm' : 'text-[#625b70] hover:text-[#6544d5]'"
                    @click="customEndMode = 'now'"
                  >
                    {{ t('historyEndNow') }}
                  </button>
                  <button
                    type="button"
                    class="flex-1 rounded-lg px-3 py-1.5 text-sm transition-colors"
                    :class="customEndMode === 'fixed' ? 'bg-white font-semibold text-[#6544d5] shadow-sm' : 'text-[#625b70] hover:text-[#6544d5]'"
                    @click="customEndMode = 'fixed'"
                  >
                    {{ t('historyEndFixed') }}
                  </button>
                </div>
                <input
                  v-if="customEndMode === 'fixed'"
                  v-model="customTo"
                  type="datetime-local"
                  class="mt-2 w-full rounded-xl border border-[#ded9ec] bg-[#faf9fd] px-3 py-2 text-sm text-[#312b3f] outline-none focus:border-[#7455e9] focus:ring-2 focus:ring-[#7455e9]/15"
                >
              </div>
              <button
                type="button"
                class="rounded-xl bg-[#7455e9] px-4 py-2 text-sm font-semibold text-white shadow-sm transition hover:bg-[#6544d5] disabled:cursor-not-allowed disabled:opacity-50 sm:col-span-2 sm:w-fit"
                :disabled="historyLoading"
                @click="applyCustomRange"
              >
                {{ t('applyHistoryRange') }}
              </button>
            </div>
            <div class="min-w-0 flex-1 text-xs text-gray-400">
              <p v-if="historyMeta">
                {{ t('historyResolution') }}：{{ formatResolution(historyMeta.resolution) }} · {{ t('historyPointInterval') }}：{{ formatInterval(historyMeta.step) }}
              </p>
              <p v-else-if="historyLoading">
                {{ t('historyLoading') }}
              </p>
              <p v-if="historyError" class="mt-1 text-red-500">
                {{ historyError }}
              </p>
            </div>
          </div>
        </section>
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
          <span class="text-xs text-gray-400">
            {{ t(hasFixedEnd ? 'historyFrozenHint' : 'historyRefreshHint') }}
          </span>
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
import type { HistoryLoadMeta, MetricKey } from '@/history'
import { appendLiveMetrics, ensureMetricsHistoryRange, localMetricsHistoryRange } from '@/history'
import { HISTORY_RANGES } from '@/historyRanges'
import type { ServerData } from '@/types'
import { formatTime, isOnline } from '@/utils'
import { formatUptime, useI18n } from '@/i18n'

const props = defineProps<{ host: string; gid?: string }>()
const { languageOptions, locale, t } = useI18n()
const rangeSelection = ref(String(60 * 60))
const customFrom = ref(toDateTimeLocal(Math.floor(Date.now() / 1000) - 60 * 60))
const customTo = ref(toDateTimeLocal(Math.floor(Date.now() / 1000)))
const customEndMode = ref<'now' | 'fixed'>('now')
const appliedRange = ref<{ from: number; to?: number }>({
  from: Math.floor(Date.now() / 1000) - 60 * 60,
})
const historyMeta = ref<HistoryLoadMeta>()
const historyError = ref('')
const historyLoading = ref(false)
let historyRequestId = 0
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
const rangeOptions = computed(() => [
  ...HISTORY_RANGES.map(range => ({ value: String(range.seconds), label: t(range.labelKey) })),
  { value: 'custom', label: t('customRange') },
])
const viewOptions = computed(() => [
  { value: 'summary', label: t('summaryView') },
  { value: 'detailed', label: t('detailedView') },
])
const isCustomRange = computed(() => rangeSelection.value === 'custom')
const hasFixedEnd = computed(() => isCustomRange.value && appliedRange.value.to !== undefined)
const selectedRangeLabel = computed(() => {
  if (!isCustomRange.value)
    return rangeOptions.value.find(item => item.value === rangeSelection.value)?.label || ''
  const end = appliedRange.value.to === undefined
    ? t('historyEndNow')
    : formatHistoryDate(appliedRange.value.to)
  return `${formatHistoryDate(appliedRange.value.from)} – ${end}`
})

const MAX_HISTORY_SECONDS = 10 * 366 * 24 * 60 * 60

function toDateTimeLocal(seconds: number) {
  const date = new Date(seconds * 1000)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}

function formatHistoryDate(seconds: number) {
  return new Intl.DateTimeFormat(locale.value, {
    year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false,
  }).format(new Date(seconds * 1000))
}

function formatInterval(seconds: number) {
  if (seconds < 60)
    return `${seconds}${t('secondsShort')}`
  if (seconds < 3600)
    return `${Math.round(seconds / 60)}${t('minutesShort')}`
  if (seconds < 86400)
    return `${Math.round(seconds / 3600)}${t('hoursShort')}`
  return `${Math.round(seconds / 86400)}${t('daysShort')}`
}

function formatResolution(resolution: string) {
  const match = resolution.match(/^(\d+)(s|m|h|d|mo)$/)
  if (!match)
    return resolution
  const multiplier = { s: 1, m: 60, h: 3600, d: 86400, mo: 30 * 86400 }[match[2]] || 1
  return formatInterval(Number(match[1]) * multiplier)
}

function applyCustomRange() {
  const from = Math.floor(new Date(customFrom.value).getTime() / 1000)
  const now = Math.floor(Date.now() / 1000)
  const to = customEndMode.value === 'now'
    ? now
    : Math.floor(new Date(customTo.value).getTime() / 1000)
  if (!Number.isFinite(from) || !Number.isFinite(to) || from >= to) {
    historyError.value = t('historyRangeInvalid')
    return
  }
  if (to - from > MAX_HISTORY_SECONDS) {
    historyError.value = t('historyRangeTooLong')
    return
  }
  if (to > now + 300) {
    historyError.value = t('historyRangeFuture')
    return
  }
  historyError.value = ''
  appliedRange.value = customEndMode.value === 'now' ? { from } : { from, to }
  refreshHistory().catch(() => undefined)
}

async function fetchServer(stopAtFixedEnd = false) {
  if (stopAtFixedEnd && hasFixedEnd.value)
    return
  const params = new URLSearchParams({ host: props.host })
  if (props.gid !== undefined)
    params.set('gid', props.gid)
  try {
    const response = await fetch(`/api/hosts?${params}`)
    if (response.ok) {
      const data = await response.json() as { updated: number; servers: ServerData[] }
      if (stopAtFixedEnd && hasFixedEnd.value)
        return
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
    if (stopAtFixedEnd && hasFixedEnd.value)
      return
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
  const requestId = ++historyRequestId
  historyLoading.value = true
  historyError.value = ''
  historyMeta.value = undefined
  const metrics = [...new Set(visiblePanels.value.flatMap(panel => panel.metrics))]
  try {
    const now = Math.floor(Date.now() / 1000)
    const custom = isCustomRange.value
    const from = custom ? appliedRange.value.from : now - Number(rangeSelection.value)
    const to = custom ? appliedRange.value.to ?? now : now
    // One complete request lets the backend choose a consistent SQLite
    // rollup tier for the interval, from seconds through monthly buckets.
    const result = await ensureMetricsHistoryRange(server.value.name, metrics, from, to, server.value.gid)
    if (requestId !== historyRequestId)
      return
    histories.value = result.histories
    historyMeta.value = result.meta
    if (!result.meta && !Object.values(result.histories).some(points => points.length > 0))
      historyError.value = t('historyLoadError')
  }
  catch {
    if (requestId === historyRequestId)
      historyError.value = t('historyLoadError')
  }
  finally {
    if (requestId === historyRequestId)
      historyLoading.value = false
  }
}

async function refresh() {
  await fetchServer()
  await refreshHistory()
  loading.value = false
}

function refreshLocalHistory() {
  if (!server.value || hasFixedEnd.value)
    return
  const metrics = [...new Set(visiblePanels.value.flatMap(panel => panel.metrics))]
  const to = Math.floor(Date.now() / 1000)
  const from = isCustomRange.value
    ? appliedRange.value.from
    : to - Number(rangeSelection.value)
  histories.value = localMetricsHistoryRange(server.value.name, metrics, from, to, server.value.gid)
}

watch(rangeSelection, (value) => {
  if (value === 'custom') {
    customFrom.value = toDateTimeLocal(appliedRange.value.from)
    customEndMode.value = appliedRange.value.to === undefined ? 'now' : 'fixed'
    customTo.value = toDateTimeLocal(appliedRange.value.to ?? Math.floor(Date.now() / 1000))
    return
  }
  const now = Math.floor(Date.now() / 1000)
  appliedRange.value = { from: now - Number(value) }
  refreshHistory().catch(() => undefined)
})
watch(viewMode, () => {
  refreshHistory().catch(() => undefined)
})
onMounted(async () => {
  await refresh()
  pollTimer.value = window.setInterval(() => {
    // An explicitly bounded interval is an immutable snapshot. Do not poll,
    // update host cards, or append browser samples until a live range is
    // selected again.
    fetchServer(true)
      .then(refreshLocalHistory)
      .catch(() => undefined)
  }, 5000)
})
onUnmounted(() => {
  if (pollTimer.value)
    window.clearInterval(pollTimer.value)
})
</script>
