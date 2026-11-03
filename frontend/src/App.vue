<template>
  <div class="min-h-screen bg-[#f7f7fb] text-[#24212d]">
    <header class="sticky top-0 z-50 border-b border-[#ebe8f2] bg-white/95 backdrop-blur">
      <div class="mx-auto flex max-w-[1800px] flex-wrap items-center justify-between gap-4 px-4 py-3 sm:px-6">
        <div class="flex items-center gap-3">
          <div class="grid h-10 w-10 place-items-center rounded-xl bg-[#7455e9] text-xl font-bold text-white shadow-lg shadow-[#7455e9]/20">
            S
          </div>
          <div>
            <h1 class="text-xl font-bold tracking-tight sm:text-2xl">
              {{ t('appTitle') }}
            </h1><p class="text-xs text-gray-400">
              ServerStatus History
            </p>
          </div>
        </div>
        <div class="flex flex-wrap items-center justify-end gap-x-5 gap-y-2 text-sm">
          <label class="flex items-center gap-2 font-medium text-gray-600">
            <span>{{ t('language') }}</span><select v-model="locale" class="rounded-lg border border-[#ded9ec] bg-white px-2 py-1.5 outline-none">
              <option v-for="option in languageOptions" :key="option.value" :value="option.value">
                {{ option.label }}
              </option>
            </select>
          </label>
          <div class="flex items-center gap-2">
            <span class="font-medium text-gray-600">{{ t('layoutMode') }}</span><div class="flex overflow-hidden rounded-lg border border-[#ded9ec] bg-white p-0.5">
              <button v-for="mode in layoutModes" :key="mode.value" class="rounded-md p-1.5 transition-colors" :class="settings.layout === mode.value ? 'bg-[#7455e9] text-white' : 'text-gray-400 hover:bg-[#f1edff] hover:text-[#6748d8]'" :title="t(mode.labelKey)" @click="settings.layout = mode.value">
                <IconLayoutGrid v-if="mode.value === 'grid'" class="h-5 w-5" /><IconLayoutFlex v-else-if="mode.value === 'flex'" class="h-5 w-5" /><IconLayoutList v-else class="h-5 w-5" />
              </button>
            </div>
          </div>
          <label class="flex items-center gap-2 font-medium text-gray-600">{{ t('compact') }}<Switch v-model="settings.compactMode" /></label>
          <label class="flex items-center gap-2 font-medium text-gray-600">{{ t('trendChart') }}<Switch v-model="settings.showMetricChart" /></label>
          <select v-if="settings.showMetricChart" v-model="settings.chartMetric" class="rounded-lg border border-[#ded9ec] bg-white px-3 py-2 outline-none" :aria-label="t('trendMetric')">
            <option v-for="option in METRIC_OPTIONS" :key="option.key" :value="option.key">
              {{ t(option.labelKey) }}
            </option>
          </select>
          <select v-if="settings.showMetricChart" v-model="settings.chartHistoryKeep" class="rounded-lg border border-[#ded9ec] bg-white px-3 py-2 outline-none" :aria-label="t('trendRange')">
            <option value="300">
              {{ t('range5Minutes') }}
            </option><option value="3600">
              {{ t('range1Hour') }}
            </option><option value="21600">
              {{ t('range6Hours') }}
            </option><option value="86400">
              {{ t('range24Hours') }}
            </option><option value="604800">
              {{ t('range7Days') }}
            </option><option value="2592000">
              {{ t('range30Days') }}
            </option>
          </select>
        </div>
      </div>
    </header>
    <div v-if="error" class="mx-auto mt-4 w-fit rounded-lg bg-red-50 px-4 py-2 text-sm text-red-700">
      {{ t('loadError') }}
    </div>
    <div class="mx-auto flex max-w-[1800px] flex-col lg:flex-row">
      <aside class="shrink-0 border-b border-[#ebe8f2] bg-white p-4 lg:sticky lg:top-[65px] lg:min-h-[calc(100vh-65px)] lg:w-64 lg:border-b-0 lg:border-r">
        <div>
          <button class="flex w-full items-center justify-between rounded-lg px-3 py-2.5 text-left text-sm" :class="settings.directoryFilter === 'all' ? 'bg-[#f1edff] font-semibold text-[#6544d5]' : 'font-semibold text-gray-700 hover:bg-[#f8f6ff]'" @click="selectDirectory('all')">
            <span class="flex min-w-0 items-center gap-2"><span class="text-[#7455e9]">▾</span><span class="truncate">{{ t('allHosts') }}</span></span><span class="ml-2 rounded-full bg-[#f5f3f9] px-2 py-0.5 text-xs text-gray-500">{{ allDirectory.count }}</span>
          </button><div class="ml-5 mt-1 space-y-1 border-l border-[#e4e0eb] pl-2">
            <button v-for="directory in directories" :key="directory.id" class="flex w-full items-center justify-between rounded-lg px-3 py-2.5 text-left text-sm" :class="settings.directoryFilter === directory.id ? 'bg-[#f1edff] font-semibold text-[#6544d5]' : 'text-gray-600 hover:bg-[#f8f6ff]'" @click="selectDirectory(directory.id)">
              <span class="flex min-w-0 items-center gap-2"><span class="text-[#a79bbf]">├</span><span class="truncate">{{ directory.name }}</span></span><span class="ml-2 rounded-full bg-[#f5f3f9] px-2 py-0.5 text-xs text-gray-500">{{ directory.count }}</span>
            </button>
          </div><p class="mt-4 rounded-lg bg-[#faf8ff] px-3 py-2 text-xs leading-5 text-gray-500">
            {{ t('directoryHelp') }}
          </p>
        </div>
        <div class="mt-6 border-t border-[#ebe8f2] pt-5">
          <div class="flex items-center justify-between px-2">
            <div class="text-xs font-bold uppercase tracking-wider text-gray-400">
              {{ t('tagFilter') }}
            </div><button v-if="settings.selectedTags.length" class="text-xs font-medium text-[#7455e9]" @click="clearTags">
              {{ t('clear') }}
            </button>
          </div><p class="mt-2 px-2 text-xs leading-5 text-gray-500">
            {{ t('tagHelp') }}
          </p><div class="mt-3 flex flex-wrap gap-2">
            <button v-for="tag in availableTags" :key="tag.name" class="rounded-lg border px-3 py-2 text-sm transition-colors" :class="isTagSelected(tag.name) ? 'border-[#7455e9] bg-[#7455e9] text-white shadow-sm' : 'border-[#e4e0eb] bg-white text-gray-600 hover:border-[#a994ee]'" @click="toggleTag(tag.name)">
              {{ tag.name }} <span class="ml-1 opacity-70">{{ tag.count }}</span><span v-if="isTagSelected(tag.name)" class="ml-1">✓</span>
            </button>
          </div><div v-if="availableTags.length === 0" class="mt-3 rounded-lg bg-gray-50 px-3 py-4 text-center text-sm text-gray-400">
            {{ t('noTags') }}
          </div>
        </div>
      </aside>
      <main class="min-w-0 flex-1 p-4 sm:p-6">
        <div class="mb-5 flex flex-wrap items-end justify-between gap-3">
          <div>
            <div class="flex items-center gap-2">
              <h2 class="text-2xl font-bold tracking-tight">
                {{ currentViewTitle }}
              </h2><span class="rounded-full bg-[#f1edff] px-2.5 py-1 text-xs font-semibold text-[#6544d5]">{{ uniqueHostCount }}</span>
            </div><p class="mt-1 text-sm text-gray-400">
              {{ viewDescription }}
            </p>
          </div><div class="flex gap-2 text-xs">
            <span class="rounded-lg border border-[#ebe8f2] bg-white px-3 py-2 text-gray-500"><strong class="text-[#24212d]">{{ onlineHostCount }}</strong> {{ t('online') }}</span><span class="rounded-lg border border-[#ebe8f2] bg-white px-3 py-2 text-gray-500"><strong class="text-[#24212d]">{{ uniqueHostCount - onlineHostCount }}</strong> {{ t('offline') }}</span>
          </div>
        </div>
        <div v-if="loading || loadingView" class="rounded-xl border border-[#ebe8f2] bg-white px-5 py-12 text-center text-gray-400">
          {{ t('loadingHosts') }}
        </div>
        <Transition v-else name="popup-bottom">
          <div class="space-y-6">
            <section v-for="group in serverGroups" :key="group.key">
              <div v-if="group.label" class="mb-3 flex items-center gap-2">
                <h3 class="text-lg font-bold">
                  {{ group.label }}
                </h3><span class="rounded-full bg-[#f1edff] px-2 py-0.5 text-xs font-medium text-[#6544d5]">{{ group.servers.length }}</span>
              </div><div :class="{ 'grid gap-4': settings.layout === 'grid', 'flex flex-wrap gap-4': settings.layout === 'flex', 'flex flex-col gap-3': settings.layout === 'list' }" :style="settings.layout === 'grid' ? { gridTemplateColumns: `repeat(${serverCardCount}, minmax(0, 1fr))` } : undefined">
                <template v-if="settings.layout === 'grid' || settings.layout === 'flex'">
                  <ServerCard v-for="server in group.servers" :key="`${group.key}:${server.name}`" :server="server" :compact-mode="settings.compactMode" :show-metric-chart="settings.showMetricChart" :chart-history="metricHistories[server.name] || []" :chart-label="t(selectedMetric.labelKey)" :chart-unit="selectedMetric.unit" :chart-decimals="selectedMetric.decimals" :chart-max="selectedMetric.max" :class="settings.layout === 'flex' ? 'min-w-[300px] flex-1' : ''" />
                </template><template v-else>
                  <ServerItem v-for="server in group.servers" :key="`${group.key}:${server.name}`" :server="server" :compact-mode="settings.compactMode" />
                </template>
              </div>
            </section><div v-if="serverGroups.length === 0" class="rounded-xl border border-[#ebe8f2] bg-white px-5 py-12 text-center text-gray-400">
              {{ t('noHosts') }}
            </div>
          </div>
        </Transition>
      </main>
    </div>
    <footer class="w-full border-t border-[#ebe8f2] bg-white py-4 text-center text-sm text-gray-400">
      {{ t('poweredBy') }} <a class="text-[#7455e9]" href="https://github.com/8-diagrams/ServerStatusHistory" target="_blank" rel="noreferrer">ServerStatus-Rust</a>.
      {{ t('theme') }} <a class="text-[#7455e9]" href="https://github.com/orilights/ServerStatus-Theme-Light" target="_blank" rel="noreferrer">Light</a> {{ t('by') }} <a class="text-[#7455e9]" href="https://orilight.top/" target="_blank" rel="noreferrer">OriLight</a>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { useLocalStorage, useWindowSize } from '@vueuse/core'
import type { ChartPoint, MetricKey } from './history'
import { METRIC_OPTIONS, appendLiveMetrics, ensureMetricHistory, localMetricHistory, metricOption } from './history'
import { useI18n } from './i18n'
import type { ServerData } from './types'

interface CatalogDirectory { id: string; name: string; count: number }
interface CatalogTag { name: string; count: number }
interface HostCatalog { default: CatalogDirectory; directories: CatalogDirectory[]; tags: CatalogTag[] }
const LEGACY_STATS_API = '/json/stats.json'
const HOSTS_API = '/api/hosts'
const CATALOG_API = '/api/hosts/catalog'
const CARD_WIDTH = 350
const MIN_FETCH_INTERVAL = 500
const HISTORY_RETRY_INTERVAL = 5 * 60 * 1000
const { width: windowWidth } = useWindowSize()
const { languageOptions, locale, t } = useI18n()
const layoutModes = [
  { value: 'grid' as const, labelKey: 'layoutGrid' as const },
  { value: 'flex' as const, labelKey: 'layoutAdaptive' as const },
  { value: 'list' as const, labelKey: 'layoutList' as const },
]
const settings = useLocalStorage('sstl-settings', { layout: 'grid' as 'grid' | 'flex' | 'list', compactMode: false, directoryFilter: 'default', selectedTags: [] as string[], showMetricChart: false, chartMetric: 'cpu' as MetricKey, chartHistoryKeep: 300 }, { mergeDefaults: true })
const catalog = ref<HostCatalog>()
const serverData = ref<{ updated: number; servers: ServerData[] }>()
const legacyAllServers = ref<ServerData[]>([])
const scopedApiAvailable = ref(false)
const loading = ref(true)
const loadingView = ref(false)
const error = ref(false)
const fetching = ref(false)
const latestUpdated = ref(0)
const timer = ref<Worker>()
const metricHistories = ref<Record<string, ChartPoint[]>>({})
const historyRequests = new Map<string, number>()
let requestVersion = 0
const serverCardCount = computed(() => Math.max(1, Math.floor((windowWidth.value - 300) / CARD_WIDTH)))
const selectedMetric = computed(() => metricOption(settings.value.chartMetric as MetricKey))
const allDirectory = computed<CatalogDirectory>(() => ({
  id: 'all',
  name: t('allHosts'),
  count: catalog.value
    ? catalog.value.default.count + catalog.value.directories.reduce((sum, item) => sum + item.count, 0)
    : 0,
}))
const directories = computed(() => {
  if (!catalog.value)
    return [{ id: 'default', name: t('defaultDirectory'), count: 0 }]
  return [{ ...catalog.value.default, name: t('defaultDirectory') }, ...catalog.value.directories]
})
const availableTags = computed(() => catalog.value?.tags || [])
const currentServers = computed(() => serverData.value?.servers || [])
const uniqueHostCount = computed(() => currentServers.value.length)
const onlineHostCount = computed(() => currentServers.value.filter(server => server.online4 || server.online6).length)
const currentDirectory = computed(() => settings.value.directoryFilter === 'all'
  ? allDirectory.value
  : directories.value.find(item => item.id === settings.value.directoryFilter))
const currentViewTitle = computed(() => currentDirectory.value?.name || t('defaultDirectory'))
const viewDescription = computed(() => settings.value.selectedTags.length
  ? t('directoryTagDescription', { tags: settings.value.selectedTags.join(' OR ') })
  : t('directoryAllDescription'))
const serverGroups = computed(() => currentServers.value.length
  ? [{ key: settings.value.directoryFilter, label: '', servers: currentServers.value }]
  : [])

onMounted(async () => {
  // Every fresh page load starts from the lightweight virtual default
  // directory. Presentation preferences such as layout remain persisted.
  settings.value.directoryFilter = 'default'
  settings.value.selectedTags = []
  await initializeData()
  timer.value = new Worker(new URL('./worker/timer.js', import.meta.url))
  timer.value.addEventListener('message', fetchData)
  timer.value.postMessage('start')
})
onUnmounted(() => {
  timer.value?.postMessage('stop')
})

async function initializeData() {
  try {
    const response = await fetch(CATALOG_API)
    if (!response.ok)
      throw new Error('catalog unavailable')
    const value = await response.json() as HostCatalog
    if (!value.default || !Array.isArray(value.directories) || !Array.isArray(value.tags))
      throw new Error('invalid catalog')
    catalog.value = value
    scopedApiAvailable.value = true
    await fetchCurrentView(true)
  }
  catch {
    scopedApiAvailable.value = false
    try {
      await fetchLegacyData()
    }
    catch {
      error.value = true
      acceptServerData({ updated: Math.floor(Date.now() / 1000), servers: [] })
    }
  }
  finally {
    loading.value = false
  }
}

function currentHostsUrl() {
  const params = new URLSearchParams()
  params.set('directory', settings.value.directoryFilter || 'default')
  settings.value.selectedTags.forEach(tag => params.append('tag', tag))
  return `${HOSTS_API}?${params.toString()}`
}

async function fetchCurrentView(showLoading = false) {
  const version = ++requestVersion
  loadingView.value = showLoading
  try {
    const response = await fetch(currentHostsUrl())
    if (!response.ok)
      throw new Error('host query failed')
    const data = await response.json()
    if (!Array.isArray(data.servers))
      throw new Error(data.message || 'invalid response')
    if (version === requestVersion)
      acceptServerData(data)
  }
  finally {
    if (version === requestVersion)
      loadingView.value = false
  }
}

async function fetchLegacyData() {
  const response = await fetch(LEGACY_STATS_API)
  if (!response.ok)
    throw new Error('legacy stats failed')
  const data = await response.json()
  legacyAllServers.value = data.servers || []
  catalog.value = createLegacyCatalog(legacyAllServers.value)
  acceptServerData({ updated: data.updated, servers: filterLegacyServers(legacyAllServers.value) })
}

function createLegacyCatalog(servers: ServerData[]): HostCatalog {
  const counts = new Map<string, number>()
  const tags = new Map<string, CatalogTag>()
  let defaultCount = 0
  for (const server of servers) {
    const gid = server.gid?.trim()
    if (gid)
      counts.set(gid, (counts.get(gid) || 0) + 1)
    else
      defaultCount++
    for (const tag of new Set(server.tags || [])) {
      const key = tag.toLocaleLowerCase()
      const item = tags.get(key) || { name: tag, count: 0 }
      item.count++
      tags.set(key, item)
    }
  }
  return {
    default: { id: 'default', name: 'default', count: defaultCount },
    directories: [...counts].map(([id, count]) => ({ id, name: id, count })),
    tags: [...tags.values()].sort((a, b) => a.name.localeCompare(b.name)),
  }
}

function filterLegacyServers(servers: ServerData[]) {
  const directory = settings.value.directoryFilter || 'default'
  const selected = new Set(settings.value.selectedTags.map(tag => tag.toLocaleLowerCase()))
  return servers.filter((server) => {
    const directoryMatches = directory === 'all'
      || (directory === 'default' ? !server.gid?.trim() : server.gid?.toLocaleLowerCase() === directory.toLocaleLowerCase())
    const tagsMatch = selected.size === 0
      || (server.tags || []).some(tag => selected.has(tag.toLocaleLowerCase()))
    return directoryMatches && tagsMatch
  })
}

async function fetchData() {
  if (fetching.value || Date.now() - latestUpdated.value < MIN_FETCH_INTERVAL)
    return
  fetching.value = true
  try {
    if (scopedApiAvailable.value) {
      await fetchCurrentView()
    }
    else {
      const response = await fetch(LEGACY_STATS_API)
      if (!response.ok)
        throw new Error('legacy stats failed')
      const data = await response.json()
      legacyAllServers.value = data.servers || []
      acceptServerData({ updated: data.updated, servers: filterLegacyServers(legacyAllServers.value) })
    }
    error.value = false
  }
  catch {
    error.value = true
  }
  finally {
    fetching.value = false
    latestUpdated.value = Date.now()
  }
}

function acceptServerData(data: { updated: number; servers: ServerData[] }) {
  serverData.value = data
  const range = Number(settings.value.chartHistoryKeep)
  const metric = settings.value.chartMetric as MetricKey
  for (const server of data.servers) {
    appendLiveMetrics(server, data.updated)
    metricHistories.value[server.name] = localMetricHistory(server.name, metric, range)
    if (settings.value.showMetricChart)
      refreshServerMetricHistory(server, metric, range).catch(() => undefined)
  }
}

function selectDirectory(directory: string) {
  settings.value.directoryFilter = directory
}
function isTagSelected(tag: string) {
  return settings.value.selectedTags.some(item => item.toLocaleLowerCase() === tag.toLocaleLowerCase())
}
function toggleTag(tag: string) {
  settings.value.selectedTags = isTagSelected(tag)
    ? settings.value.selectedTags.filter(item => item.toLocaleLowerCase() !== tag.toLocaleLowerCase())
    : [...settings.value.selectedTags, tag]
}
function clearTags() {
  settings.value.selectedTags = []
}

async function refreshServerMetricHistory(server: ServerData, metric: MetricKey, range: number, force = false) {
  const key = `${server.name}:${metric}:${range}`
  const previous = historyRequests.get(key) || 0
  if (!force && Date.now() - previous < HISTORY_RETRY_INTERVAL)
    return
  historyRequests.set(key, Date.now())
  metricHistories.value[server.name] = await ensureMetricHistory(server.name, metric, range)
}
async function refreshMetricHistory() {
  if (!serverData.value || !settings.value.showMetricChart)
    return
  const range = Number(settings.value.chartHistoryKeep)
  const metric = settings.value.chartMetric as MetricKey
  await Promise.all(serverData.value.servers.map(server => refreshServerMetricHistory(server, metric, range, true)))
}

watch(() => [settings.value.directoryFilter, settings.value.selectedTags.join('\u0000')], async () => {
  if (loading.value)
    return
  if (scopedApiAvailable.value) {
    await fetchCurrentView(true).catch(() => {
      error.value = true
    })
  }
  else {
    acceptServerData({ updated: Math.floor(Date.now() / 1000), servers: filterLegacyServers(legacyAllServers.value) })
  }
})
watch(() => [settings.value.showMetricChart, settings.value.chartMetric, settings.value.chartHistoryKeep], refreshMetricHistory)
</script>

<style>
.popup-bottom-enter-active, .popup-bottom-leave-active { transition: all 0.25s ease; }
.popup-bottom-enter-from, .popup-bottom-leave-to { opacity: 0; transform: translateY(8px); }
</style>
